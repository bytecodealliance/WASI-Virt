use anyhow::{bail, Context, Result};
use serde::Deserialize;
use walrus::{
    ir::Value, ActiveData, ActiveDataLocation, DataKind, ExportItem, GlobalKind, InitExpr, Module,
};
use crate::{
    walrus_ops::{
        bump_stack_global, get_active_data_segment, stub_imported_func,
    },
    WasiVirt,
};

#[derive(Deserialize, Debug, Clone, Default)]
pub struct VirtEnv {
    /// Set specific environment variable overrides
    overrides: Vec<(String, String)>,
    /// Define how to embed into the host environment
    /// (Pass-through / encapsulate / allow / deny)
    host: HostEnv,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "snake_case")]
pub enum HostEnv {
    /// Apart from the overrides, pass through all environment
    /// variables from the host
    #[default]
    All,
    /// Fully encapsulate the environment, removing all host
    /// environment import checks
    None,
    /// Only allow the provided environment variable keys
    Allow(Vec<String>),
    /// Allow all environment variables, except the provided keys
    Deny(Vec<String>),
}

impl WasiVirt {
    fn get_or_create_env(&mut self) -> &mut VirtEnv {
        self.virt_opts.env.get_or_insert_with(Default::default)
    }

    pub fn env_host_allow(mut self, allow_list: &[&str]) -> Self {
        let env = self.get_or_create_env();
        env.host = HostEnv::Allow(allow_list.iter().map(|s| s.to_string()).collect());
        self
    }

    pub fn env_host_deny(mut self, deny_list: &[&str]) -> Self {
        let env = self.get_or_create_env();
        env.host = HostEnv::Deny(deny_list.iter().map(|s| s.to_string()).collect());
        self
    }

    pub fn env_host_all(mut self) -> Self {
        let env = self.get_or_create_env();
        env.host = HostEnv::All;
        self
    }

    pub fn env_host_none(mut self) -> Self {
        let env = self.get_or_create_env();
        env.host = HostEnv::None;
        self
    }

    pub fn env_overrides(mut self, overrides: &[(&str, &str)]) -> Self {
        let env = self.get_or_create_env();
        for (key, val) in overrides {
            env.overrides.push((key.to_string(), val.to_string()));
        }
        self
    }
}

pub fn create_env_virt<'a>(module: &'a mut Module, env: &VirtEnv) -> Result<()> {
    let env_ptr_addr = {
        let env_ptr_export = module
            .exports
            .iter()
            .find(|expt| expt.name.as_str() == "env")
            .context("Adapter 'env' is not exported")?;
        let ExportItem::Global(env_ptr_global) = env_ptr_export.item else {
            bail!("Adapter 'env' not a global");
        };
        let GlobalKind::Local(InitExpr::Value(Value::I32(env_ptr_addr))) =
            &module.globals.get(env_ptr_global).kind
        else {
            bail!("Adapter 'env' not a local I32 global value");
        };
        *env_ptr_addr as u32
    };

    // If host env is disabled, remove its import entirely
    // replacing it with a stub panic
    if matches!(env.host, HostEnv::None) {
        stub_imported_func(module, "wasi:cli-base/environment", "get-environment")?;
        // we do arguments as well because virt assumes reactors for now...
        stub_imported_func(module, "wasi:cli-base/environment", "get-arguments")?;
    }

    let memory = module
        .memories
        .iter()
        .nth(0)
        .context("Adapter does not export a memory")?
        .id();

    // prepare the field data list vector for writing
    // strings must be sorted as binary searches are used against this data
    let mut field_data_vec: Vec<&str> = Vec::new();
    let mut sorted_overrides = env.overrides.clone();
    sorted_overrides.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (key, value) in &sorted_overrides {
        field_data_vec.push(key.as_ref());
        field_data_vec.push(value.as_ref());
    }
    match &env.host {
        HostEnv::Allow(allow_list) => {
            let mut allow_list: Vec<&str> = allow_list.iter().map(|item| item.as_ref()).collect();
            allow_list.sort();
            for key in allow_list {
                field_data_vec.push(key);
            }
        }
        HostEnv::Deny(deny_list) => {
            let mut deny_list: Vec<&str> = deny_list.iter().map(|item| item.as_ref()).collect();
            deny_list.sort();
            for key in deny_list {
                field_data_vec.push(key);
            }
        }
        _ => {}
    }

    let mut field_data_bytes = Vec::new();
    for str in field_data_vec {
        assert!(field_data_bytes.len() % 4 == 0);
        // write the length at the aligned offset
        field_data_bytes.extend_from_slice(&(str.len() as u32).to_le_bytes());
        let str_bytes = str.as_bytes();
        field_data_bytes.extend_from_slice(str_bytes);
        let rem = str_bytes.len() % 4;
        // add padding for alignment if necessary
        if rem > 0 {
            field_data_bytes.extend((0..4 - rem).map(|_| 0));
        }
    }

    let field_data_addr = if field_data_bytes.len() > 0 {
        // Offset the stack global by the static field data length
        let field_data_addr = bump_stack_global(module, field_data_bytes.len() as i32)?;

        // Add a new data segment for this new range created at the top of the stack
        module.data.add(
            DataKind::Active(ActiveData {
                memory,
                location: ActiveDataLocation::Absolute(field_data_addr),
            }),
            field_data_bytes,
        );
        Some(field_data_addr)
    } else {
        None
    };

    // In the existing static data segment, update the static data options.
    //
    // From virtual-adapter/src/lib.js:
    //
    // #[repr(C)]
    // pub struct Env {
    //     /// Whether to fallback to the host env
    //     /// [byte 0]
    //     host_fallback: bool,
    //     /// Whether we are providing an allow list or a deny list
    //     /// on the fallback lookups
    //     /// [byte 1]
    //     host_fallback_allow: bool,
    //     /// How many host fields are defined in the data pointer
    //     /// [byte 4]
    //     host_field_cnt: u32,
    //     /// Host many host fields are defined to be allow or deny
    //     /// (these are concatenated at the end of the data with empty values)
    //     /// [byte 8]
    //     host_allow_or_deny_cnt: u32,
    //     /// Byte data of u32 byte len followed by string bytes
    //     /// up to the lengths previously provided.
    //     /// [byte 12]
    //     host_field_data: *const u8,
    // }
    let (data, data_offset) = get_active_data_segment(module, memory, env_ptr_addr)?;
    let bytes = data.value.as_mut_slice();

    let host_field_cnt = env.overrides.len() as u32;
    bytes[data_offset + 4..data_offset + 8].copy_from_slice(&host_field_cnt.to_le_bytes());
    match &env.host {
        // All is already the default data
        HostEnv::All => {}
        HostEnv::None => {
            bytes[data_offset] = 0;
        }
        HostEnv::Allow(allow_list) => {
            bytes[data_offset + 1] = 1;
            bytes[data_offset + 8..data_offset + 12]
                .copy_from_slice(&(allow_list.len() as u32).to_le_bytes());
        }
        HostEnv::Deny(deny_list) => {
            bytes[data_offset + 1] = 0;
            bytes[data_offset + 8..data_offset + 12]
                .copy_from_slice(&(deny_list.len() as u32).to_le_bytes());
        }
    };
    if let Some(field_data_addr) = field_data_addr {
        bytes[data_offset + 12..data_offset + 16].copy_from_slice(&field_data_addr.to_le_bytes());
    }

    Ok(())
}
