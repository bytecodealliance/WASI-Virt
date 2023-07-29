use anyhow::{Context, Result};
use serde::Deserialize;
use std::env;
use std::fs;
use std::time::SystemTime;
use virt_env::{create_env_virt, strip_env_virt};
use virt_io::strip_clocks_virt;
use virt_io::strip_fs_virt;
use virt_io::strip_http_virt;
use virt_io::strip_stdio_virt;
use virt_io::VirtStdio;
use virt_io::{create_io_virt, strip_io_virt};
use walrus::ValType;
use walrus_ops::add_stub_exported_func;
use wasm_metadata::Producers;
use wasm_opt::Feature;
use wasm_opt::OptimizationOptions;
use wit_component::metadata;
use wit_component::ComponentEncoder;
use wit_component::StringEncoding;

mod data;
mod virt_env;
mod virt_io;
mod walrus_ops;

pub use virt_env::{HostEnv, VirtEnv};
pub use virt_io::{FsEntry, VirtFs, VirtualFiles};

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum VirtExit {
    #[default]
    Unreachable,
    Passthrough,
}

/// Virtualization options
///
/// When subsystems are not virtualized, their imports and exports
/// are simply ignored by the virtualizer, allowing for the creation
/// of subsystem-specific virtualizations.
///
/// Note: The default virtualization for all virtualization modes is
/// full encapsulation.
///
#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct WasiVirt {
    /// Environment virtualization
    pub env: Option<VirtEnv>,
    /// Filesystem virtualization
    pub fs: Option<VirtFs>,
    /// Stdio virtualization
    pub stdio: Option<VirtStdio>,
    /// Exit virtualization
    pub exit: Option<VirtExit>,
    /// Clocks virtualization
    #[serde(default)]
    pub clocks: bool,
    /// Http virtualization
    #[serde(default)]
    pub http: bool,
    /// Disable wasm-opt run if desired
    pub wasm_opt: Option<bool>,
}

pub struct VirtResult {
    pub adapter: Vec<u8>,
    pub virtual_files: VirtualFiles,
}

impl WasiVirt {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clocks(&mut self) {
        self.clocks = true;
    }

    pub fn http(&mut self) {
        self.http = true;
    }

    pub fn exit(&mut self, virt_exit: VirtExit) {
        self.exit = Some(virt_exit);
    }

    pub fn env(&mut self) -> &mut VirtEnv {
        self.env.get_or_insert_with(Default::default)
    }

    pub fn fs(&mut self) -> &mut VirtFs {
        self.fs.get_or_insert_with(Default::default)
    }

    pub fn stdio(&mut self) -> &mut VirtStdio {
        self.stdio.get_or_insert_with(Default::default)
    }

    pub fn opt(&mut self, opt: bool) {
        self.wasm_opt = Some(opt);
    }

    pub fn finish(&mut self) -> Result<VirtResult> {
        let virt_adapter = include_bytes!("../lib/virtual_adapter.wasm");

        let config = walrus::ModuleConfig::new();
        let mut module = config.parse(virt_adapter)?;
        module.name = Some("wasi_virt".into());

        // very few subsystems are fully independent of io, these are them
        if let Some(env) = &self.env {
            create_env_virt(&mut module, env)?;
        }
        if matches!(self.exit, Some(VirtExit::Unreachable)) {
            add_stub_exported_func(
                &mut module,
                "wasi:cli-base/exit#exit",
                vec![ValType::I32],
                vec![],
            )?;
        }

        let has_io = self.fs.is_some() || self.stdio.is_some();

        let virtual_files = if has_io {
            // io virt is managed through a singular io configuration
            create_io_virt(&mut module, self.fs.as_ref(), self.stdio.as_ref())?
        } else {
            Default::default()
        };

        // decode the component custom section to strip out the unused world exports
        // before reencoding.
        let mut component_section = module
            .customs
            .remove_raw("component-type:virtual-adapter")
            .context("Unable to find component section")?;

        let (_, mut bindgen) = metadata::decode(virt_adapter)?;
        let (_, pkg_id) = bindgen
            .resolve
            .package_names
            .iter()
            .find(|(name, _)| name.namespace == "local")
            .unwrap();

        let base_world = bindgen
            .resolve
            .select_world(*pkg_id, Some("virtual-base"))?;

        let env_world = bindgen.resolve.select_world(*pkg_id, Some("virtual-env"))?;
        let io_world = bindgen.resolve.select_world(*pkg_id, Some("virtual-io"))?;
        // let exit_world = bindgen
        //     .resolve
        //     .select_world(*pkg_id, Some("virtual-exit"))?;
        let fs_world = bindgen.resolve.select_world(*pkg_id, Some("virtual-fs"))?;
        let stdio_world = bindgen
            .resolve
            .select_world(*pkg_id, Some("virtual-stdio"))?;
        let clocks_world = bindgen
            .resolve
            .select_world(*pkg_id, Some("virtual-clocks"))?;
        let http_world = bindgen
            .resolve
            .select_world(*pkg_id, Some("virtual-http"))?;

        if self.env.is_some() {
            bindgen.resolve.merge_worlds(env_world, base_world)?;
        } else {
            strip_env_virt(&mut module)?;
        }
        if has_io {
            bindgen.resolve.merge_worlds(io_world, base_world)?;

            // io subsystems have io dependence due to streams + poll
            if self.clocks {
                bindgen.resolve.merge_worlds(clocks_world, base_world)?;
            } else {
                strip_clocks_virt(&mut module)?;
            }
            if self.http {
                bindgen.resolve.merge_worlds(http_world, base_world)?;
            } else {
                strip_http_virt(&mut module)?;
            }
            if self.stdio.is_some() {
                bindgen.resolve.merge_worlds(stdio_world, base_world)?;
            } else {
                strip_stdio_virt(&mut module)?;
            }
            if self.fs.is_some() {
                bindgen.resolve.merge_worlds(fs_world, base_world)?;
            } else {
                strip_fs_virt(&mut module)?;
            }
        } else {
            strip_io_virt(&mut module)?;
        }

        let mut producers = Producers::default();
        producers.add("processed-by", "wasi-virt", env!("CARGO_PKG_VERSION"));

        component_section.data = metadata::encode(
            &bindgen.resolve,
            base_world,
            StringEncoding::UTF8,
            Some(&producers),
        )?;

        module.customs.add(component_section);

        let mut bytes = module.emit_wasm();

        // because we rely on dead code ellimination to remove unnecessary adapter code
        // we save into a temporary file and run wasm-opt before returning
        // this can be disabled with wasm_opt: false
        if self.wasm_opt.unwrap_or(true) {
            let dir = env::temp_dir();
            let tmp_input = dir.join(format!("virt.core.input.{}.wasm", timestamp()));
            let tmp_output = dir.join(format!("virt.core.output.{}.wasm", timestamp()));
            fs::write(&tmp_input, bytes)
                .with_context(|| "Unable to write temporary file for wasm-opt call on adapter")?;
            OptimizationOptions::new_optimize_for_size_aggressively()
                .enable_feature(Feature::ReferenceTypes)
                .run(&tmp_input, &tmp_output)
                .with_context(|| "Unable to apply wasm-opt optimization to virt. This can be disabled with wasm_opt: false.")
                .or_else(|e| {
                    fs::remove_file(&tmp_input)?;
                    Err(e)
                })?;
            bytes = fs::read(&tmp_output)?;
            fs::remove_file(&tmp_input)?;
            fs::remove_file(&tmp_output)?;
        }

        // now adapt the virtualized component
        let encoder = ComponentEncoder::default().validate(true).module(&bytes)?;
        let encoded = encoder.encode()?;

        Ok(VirtResult {
            adapter: encoded,
            virtual_files,
        })
    }
}

fn timestamp() -> u64 {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(n) => n.as_secs(),
        Err(_) => panic!(),
    }
}
