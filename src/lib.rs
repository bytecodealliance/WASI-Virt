use anyhow::{anyhow, Result};
use serde::Deserialize;
use walrus::{
    ir::{Const, Instr, Value},
    ActiveData, ActiveDataLocation, Data, DataKind, ExportItem, Function, FunctionBuilder,
    FunctionKind, GlobalKind, ImportKind, ImportedFunction, InitExpr, MemoryId, Module,
};
use wit_component::ComponentEncoder;

#[derive(Deserialize, Debug, Default, Clone)]
pub struct VirtOpts {
    /// Environment virtualization
    env: Option<VirtEnv>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct VirtEnv {
    /// Set specific environment variable overrides
    overrides: Vec<(String, String)>,
    /// Define how to embed into the host environment
    /// (Pass-through / encapsulate / allow / deny)
    host: HostEnv,
}

#[derive(Deserialize, Debug, Clone, Default)]
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

fn get_active_data_segment<'a>(
    module: &'a mut Module,
    mem: MemoryId,
    addr: u32,
) -> Result<(&'a mut Data, usize)> {
    let data = match module.data.iter().find(|&data| {
        let DataKind::Active(active_data) = &data.kind else {
            return false;
        };
        if active_data.memory != mem {
            return false;
        };
        let ActiveDataLocation::Absolute(loc) = &active_data.location else {
            return false;
        };
        *loc <= addr && *loc + data.value.len() as u32 > addr
    }) {
        Some(data) => data,
        None => {
            return Err(anyhow!("Unable to find data section for env ptr"));
        }
    };
    let DataKind::Active(ActiveData {
        location: ActiveDataLocation::Absolute(loc),
        ..
    }) = &data.kind
    else {
        unreachable!()
    };
    let data_id = data.id();
    let offset = (addr - *loc) as usize;
    Ok((module.data.get_mut(data_id), offset))
}

fn bump_stack_global<'a>(module: &'a mut Module, offset: i32) -> Result<u32> {
    let stack_global_id = match module.globals.iter().find(|&global| {
        if let Some(name) = &global.name {
            name == "__stack_pointer"
        } else {
            false
        }
    }) {
        Some(global) => global.id(),
        None => {
            return Err(anyhow!("Unable to find __stack_pointer global name"));
        }
    };
    let stack_global = module.globals.get_mut(stack_global_id);
    let GlobalKind::Local(InitExpr::Value(Value::I32(stack_value))) = &mut stack_global.kind else {
        return Err(anyhow!("Stack global is not a constant I32"));
    };
    if offset > *stack_value {
        return Err(anyhow!(
            "Stack size {} is smaller than the offset {offset}",
            *stack_value
        ));
    }
    let new_stack_value = *stack_value - offset;
    *stack_value = new_stack_value;
    Ok(new_stack_value as u32)
}

fn stub_imported_func<'a>(module: &'a mut Module, import_module: &str, import_name: &str) {
    let imported_fn = module
        .imports
        .iter()
        .find(|impt| impt.module == import_module && impt.name == import_name)
        .unwrap();

    let fid = match imported_fn.kind {
        ImportKind::Function(fid) => fid,
        _ => panic!(),
    };
    let tid = match module.funcs.get(fid) {
        Function {
            kind: FunctionKind::Import(ImportedFunction { ty, .. }),
            ..
        } => *ty,
        _ => panic!(),
    };

    let ty = module.types.get(tid);
    let (params, results) = (ty.params().to_vec(), ty.results().to_vec());

    let mut builder = FunctionBuilder::new(&mut module.types, &params, &results);
    builder.func_body().unreachable();
    let local_func = builder.local_func(vec![]);

    // substitute the local func into the imported func id
    let func = module.funcs.get_mut(fid);
    func.kind = FunctionKind::Local(local_func);

    // remove the import
    module.imports.delete(imported_fn.id());
}

pub struct WasiVirt {
    virt_opts: VirtOpts,
}

impl WasiVirt {
    pub fn new() -> Self {
        WasiVirt {
            virt_opts: VirtOpts::default(),
        }
    }

    fn get_or_create_env<'a>(&'a mut self) -> &'a mut VirtEnv {
        if self.virt_opts.env.is_none() {
            self.virt_opts.env = Some(VirtEnv::default());
        }
        self.virt_opts.env.as_mut().unwrap()
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

    pub fn create(&self) -> Result<Vec<u8>> {
        create_virt(&self.virt_opts)
    }
}

pub fn create_virt<'a>(opts: &VirtOpts) -> Result<Vec<u8>> {
    let virt_adapter = include_bytes!("../lib/virtual_adapter.wasm");

    let config = walrus::ModuleConfig::new();
    let mut module = config.parse(virt_adapter)?;

    // env virtualization injection
    if let Some(env) = &opts.env {
        let env_ptr_addr = {
            let env_ptr_export = module
                .exports
                .iter()
                .find(|expt| expt.name.as_str() == "get_env_ptr")
                .unwrap();
            let ExportItem::Function(func) = env_ptr_export.item else {
                panic!()
            };
            let FunctionKind::Local(local_func) = &module.funcs.get(func).kind else {
                panic!()
            };
            let func_body = local_func.block(local_func.entry_block());
            if func_body.instrs.len() != 1 {
                return Err(anyhow!(
                    "Unexpected get_env_ptr implementation. Should be a constant address return."
                ));
            }
            let Instr::Const(Const {
                value: Value::I32(env_ptr_addr),
            }) = &func_body.instrs[0].0
            else {
                return Err(anyhow!(
                    "Unexpected get_env_ptr implementation. Should be a constant address return."
                ));
            };
            *env_ptr_addr as u32
        };

        // If host env is disabled, remove its import entirely
        // replacing it with a stub panic
        if matches!(env.host, HostEnv::None) {
            stub_imported_func(&mut module, "wasi:cli-base/environment", "get-environment");
            // we do arguments as well because virt assumes reactors for now...
            stub_imported_func(&mut module, "wasi:cli-base/environment", "get-arguments");
        }

        let memory = module.memories.iter().nth(0).unwrap().id();

        // prepare the field data list vector for writing
        // strings must be sorted as binary searches are used against this data
        let mut field_data_vec: Vec<&str> = Vec::new();
        for (key, value) in &env.overrides {
            field_data_vec.push(key.as_ref());
            field_data_vec.push(value.as_ref());
        }
        field_data_vec.sort();
        match &env.host {
            HostEnv::Allow(allow_list) => {
                let mut allow_list: Vec<&str> =
                    allow_list.iter().map(|item| item.as_ref()).collect();
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
            let field_data_addr = bump_stack_global(&mut module, field_data_bytes.len() as i32)?;

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
        let (data, data_offset) = get_active_data_segment(&mut module, memory, env_ptr_addr)?;
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
            bytes[data_offset + 12..data_offset + 16]
                .copy_from_slice(&field_data_addr.to_le_bytes());
        }
    }

    let bytes = module.emit_wasm();

    // now adapt the virtualized component
    let encoder = ComponentEncoder::default().validate(true).module(&bytes)?;
    Ok(encoder.encode()?)
}
