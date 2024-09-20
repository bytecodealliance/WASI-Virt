use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{env, fs, path::PathBuf, time::SystemTime};
use virt_config::{create_config_virt, strip_config_virt};
use virt_deny::{
    deny_clocks_virt, deny_exit_virt, deny_http_virt, deny_random_virt, deny_sockets_virt,
};
use virt_env::{create_env_virt, strip_env_virt};
use virt_io::{create_io_virt, VirtStdio};
use walrus_ops::strip_virt;
use wasm_compose::composer::ComponentComposer;
use wasm_metadata::Producers;
use wit_component::{metadata, ComponentEncoder, DecodedWasm, StringEncoding};
use wit_parser::WorldItem;

mod data;
mod stub_preview1;
mod virt_config;
mod virt_deny;
mod virt_env;
mod virt_io;
mod walrus_ops;

pub use stub_preview1::stub_preview1;
pub use virt_config::{HostConfig, VirtConfig};
pub use virt_env::{HostEnv, VirtEnv};
pub use virt_io::{FsEntry, StdioCfg, VirtFs, VirtualFiles};

const VIRT_ADAPTER: &[u8] = include_bytes!("../lib/virtual_adapter.wasm");
const VIRT_ADAPTER_DEBUG: &[u8] = include_bytes!("../lib/virtual_adapter.debug.wasm");
const VIRT_WIT_METADATA: &[u8] = include_bytes!("../lib/package.wasm");

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
    /// Debug mode traces all virt calls
    #[serde(default)]
    pub debug: bool,
    /// Environment virtualization
    pub env: Option<VirtEnv>,
    /// Configuration virtualization
    pub config: Option<VirtConfig>,
    /// Filesystem virtualization
    pub fs: Option<VirtFs>,
    /// Stdio virtualization
    pub stdio: Option<VirtStdio>,
    /// Exit virtualization
    pub exit: Option<bool>,
    /// Clocks virtualization
    pub clocks: Option<bool>,
    /// Http virtualization
    pub http: Option<bool>,
    /// Sockets virtualization
    pub sockets: Option<bool>,
    /// Random virtualization
    pub random: Option<bool>,
    /// Disable wasm-opt run if desired
    pub wasm_opt: Option<bool>,
    /// Path to compose component
    pub compose: Option<String>,
}

pub struct VirtResult {
    pub adapter: Vec<u8>,
    pub virtual_files: VirtualFiles,
}

impl WasiVirt {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow_all(&mut self) {
        self.clocks(true);
        self.http(true);
        self.sockets(true);
        self.exit(true);
        self.random(true);
        self.env().allow_all();
        // TODO enable once wasi:config/runtime is stable
        // self.config().allow_all();
        self.fs().allow_host_preopens();
        self.stdio().allow();
    }

    pub fn deny_all(&mut self) {
        self.clocks(false);
        self.http(false);
        self.sockets(false);
        self.exit(false);
        self.random(false);
        self.env().deny_all();
        // TODO enable once wasi:config/runtime is stable
        // self.config().deny_all();
        self.fs().deny_host_preopens();
        self.stdio().ignore();
    }

    pub fn clocks(&mut self, allow: bool) {
        self.clocks = Some(allow);
    }

    pub fn http(&mut self, allow: bool) {
        self.http = Some(allow);
    }

    pub fn sockets(&mut self, allow: bool) {
        self.sockets = Some(allow);
    }

    pub fn exit(&mut self, allow: bool) {
        self.exit = Some(allow);
    }

    pub fn random(&mut self, allow: bool) {
        self.random = Some(allow);
    }

    pub fn env(&mut self) -> &mut VirtEnv {
        self.env.get_or_insert_with(Default::default)
    }

    pub fn config(&mut self) -> &mut VirtConfig {
        self.config.get_or_insert_with(Default::default)
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

    pub fn compose(&mut self, compose: String) {
        self.compose = Some(compose);
    }

    /// drop capabilities that are not imported by the composed component
    pub fn filter_imports(&mut self) -> Result<()> {
        match &self.compose {
            Some(compose) => {
                let imports = {
                    let module_bytes = fs::read(compose).map_err(anyhow::Error::new)?;
                    let (resolve, world_id) = match wit_component::decode(&module_bytes)? {
                        DecodedWasm::WitPackage(..) => {
                            bail!("expected a component, found a WIT package")
                        }
                        DecodedWasm::Component(resolve, world_id) => (resolve, world_id),
                    };

                    let mut import_ids: Vec<String> = vec![];
                    for (_, import) in &resolve.worlds[world_id].imports {
                        if let WorldItem::Interface { id, .. } = import {
                            if let Some(id) = resolve.id_of(*id) {
                                import_ids.push(id);
                            }
                        }
                    }

                    import_ids
                };
                let matches = |prefix: &str| imports.iter().any(|i| i.starts_with(prefix));

                if !matches("wasi:cli/environment") {
                    self.env = None;
                }
                if !matches("wasi:config/runtime") {
                    self.config = None;
                }
                if !matches("wasi:filesystem/") {
                    self.fs = None;
                }
                if !(matches("wasi:cli/std") || matches("wasi:cli/terminal")) {
                    self.stdio = None;
                }
                if !matches("wasi:cli/exit") {
                    self.exit = None;
                }
                if !matches("wasi:clocks/") {
                    self.clocks = None;
                }
                if !matches("wasi:http/") {
                    self.http = None;
                }
                if !matches("wasi:sockets/") {
                    self.sockets = None;
                }
                if !matches("wasi:random/") {
                    self.random = None;
                }

                Ok(())
            }
            None => bail!("filtering imports can only be applied to composed components"),
        }
    }

    pub fn finish(&mut self) -> Result<VirtResult> {
        let mut config = walrus::ModuleConfig::new();
        config.generate_name_section(self.debug);
        let mut module = if self.debug {
            config.parse(VIRT_ADAPTER_DEBUG)
        } else {
            config.parse(VIRT_ADAPTER)
        }
        .context("failed to parse adapter")?;
        module.name = Some("wasi_virt".into());

        // only env virtualization is independent of io
        if let Some(env) = &self.env {
            create_env_virt(&mut module, env).context("failed to virtualize environment")?;
        }
        if let Some(config) = &self.config {
            create_config_virt(&mut module, config).context("failed to virtualize config")?;
        }

        let has_io = self.fs.is_some()
            || self.stdio.is_some()
            || self.clocks.is_some()
            || self.http.is_some()
            || self.sockets.is_some();

        let virtual_files = if has_io {
            // io virt is managed through a singular io configuration
            create_io_virt(&mut module, self.fs.as_ref(), self.stdio.as_ref())
                .context("failed to virtualize I/O")?
        } else {
            Default::default()
        };

        let component_type_section_id = module
            .customs
            .iter()
            .find_map(|(id, section)| {
                let name = section.name();
                if name.starts_with("component-type:")
                    && section.as_any().is::<walrus::RawCustomSection>()
                {
                    Some(id)
                } else {
                    None
                }
            })
            .context("Unable to find component type section")?;

        // decode the component custom section to strip out the unused world exports
        // before reencoding.
        let mut component_section = *module
            .customs
            .delete(component_type_section_id)
            .context("Unable to find component section")?
            .into_any()
            .downcast::<walrus::RawCustomSection>()
            .unwrap();

        let (mut resolve, pkg_id) = match wit_component::decode(VIRT_WIT_METADATA)
            .context("failed to decode WIT package")?
        {
            DecodedWasm::WitPackage(resolve, pkg_id) => (resolve, pkg_id),
            DecodedWasm::Component(..) => {
                bail!("expected a WIT package, found a component")
            }
        };

        let base_world = resolve
            .select_world(pkg_id, Some("virtual-base"))
            .context("failed to select `virtual-base` world")?;

        let env_world = resolve
            .select_world(pkg_id, Some("virtual-env"))
            .context("failed to select `virtual-env` world")?;
        let config_world = resolve
            .select_world(pkg_id, Some("virtual-config"))
            .context("failed to select `virtual-config` world")?;

        let io_world = resolve
            .select_world(pkg_id, Some("virtual-io"))
            .context("failed to select `virtual-io` world")?;
        let io_clocks_world = resolve
            .select_world(pkg_id, Some("virtual-io-clocks"))
            .context("failed to select `virtual-io-clocks` world")?;
        let io_http_world = resolve
            .select_world(pkg_id, Some("virtual-io-http"))
            .context("failed to select `virtual-io-http` world")?;
        let io_sockets_world = resolve
            .select_world(pkg_id, Some("virtual-io-sockets"))
            .context("failed to select `virtual-io-sockets` world")?;

        let exit_world = resolve
            .select_world(pkg_id, Some("virtual-exit"))
            .context("failed to select `virtual-exit` world")?;
        let fs_world = resolve
            .select_world(pkg_id, Some("virtual-fs"))
            .context("failed to select `virtual-fs` world")?;
        let random_world = resolve
            .select_world(pkg_id, Some("virtual-random"))
            .context("failed to select `virtual-random` world")?;
        let stdio_world = resolve
            .select_world(pkg_id, Some("virtual-stdio"))
            .context("failed to select `virtual-stdio` world")?;
        let clocks_world = resolve
            .select_world(pkg_id, Some("virtual-clocks"))
            .context("failed to select `virtual-clocks` world")?;
        let http_world = resolve
            .select_world(pkg_id, Some("virtual-http"))
            .context("failed to select `virtual-http` world")?;
        let sockets_world = resolve
            .select_world(pkg_id, Some("virtual-sockets"))
            .context("failed to select `virtual-sockets` world")?;

        // env, config, exit & random subsystems are fully independent
        if self.env.is_some() {
            resolve
                .merge_worlds(env_world, base_world)
                .context("failed to merge with environment world")?;
        } else {
            strip_env_virt(&mut module).context("failed to strip environment exports")?;
        }
        if self.config.is_some() {
            resolve
                .merge_worlds(config_world, base_world)
                .context("failed to merge with config world")?;
        } else {
            strip_config_virt(&mut module).context("failed to strip config exports")?;
        }
        if let Some(exit) = self.exit {
            if !exit {
                resolve
                    .merge_worlds(exit_world, base_world)
                    .context("failed to merge with exit world")?;
                deny_exit_virt(&mut module).context("failed to deny exit exports")?;
            }
        }
        if let Some(random) = self.random {
            if !random {
                resolve
                    .merge_worlds(random_world, base_world)
                    .context("failed to merge with random world")?;
                deny_random_virt(&mut module).context("failed to deny random exports")?;
            }
        }

        // io subsystems have io dependence due to streams + poll
        // therefore we need to strip just their io dependence portion
        if has_io {
            resolve
                .merge_worlds(io_world, base_world)
                .context("failed to merge with I/O world")?;
        } else {
            strip_virt(&mut module, &["wasi:io/"]).context("failed to strip I/O exports")?;
        }
        if let Some(clocks) = self.clocks {
            if !clocks {
                // deny is effectively virtualization
                // in future with fine-grained virtualization options, they
                // also would extend here (ie !clocks is deceiving)
                resolve
                    .merge_worlds(clocks_world, base_world)
                    .context("failed to merge with clock world")?;
                deny_clocks_virt(&mut module).context("failed to deny clock exports")?;
            } else {
                // passthrough can be simplified to just rewrapping io interfaces
                resolve
                    .merge_worlds(io_clocks_world, base_world)
                    .context("failed to merge I/O clocks world")?;
            }
        } else {
            strip_virt(&mut module, &["wasi:clocks/"]).context("failed to strip clock exports")?;
        }
        // sockets and http are identical to clocks above
        if let Some(sockets) = self.sockets {
            if !sockets {
                resolve
                    .merge_worlds(sockets_world, base_world)
                    .context("failed to merge with sockets world")?;
                deny_sockets_virt(&mut module).context("failed to deny socket exports")?;
            } else {
                resolve
                    .merge_worlds(io_sockets_world, base_world)
                    .context("failed to merge with socket I/O world")?;
            }
        } else {
            strip_virt(&mut module, &["wasi:sockets/"])
                .context("failed to strip socket exports")?;
        }
        if let Some(http) = self.http {
            if !http {
                resolve
                    .merge_worlds(http_world, base_world)
                    .context("failed to merge with HTTP world")?;
                deny_http_virt(&mut module).context("failed to deny with HTTP exports")?;
            } else {
                resolve
                    .merge_worlds(io_http_world, base_world)
                    .context("failed to merge with HTTP I/O world")?;
            }
        } else {
            strip_virt(&mut module, &["wasi:http/"]).context("failed to strip HTTP exports")?;
        }

        // stdio and fs are fully implemented in io world
        // (all their interfaces use streams)
        if self.stdio.is_some() {
            resolve
                .merge_worlds(stdio_world, base_world)
                .context("failed to merge with stdio world")?;
        } else {
            strip_virt(&mut module, &["wasi:cli/std", "wasi:cli/terminal"])
                .context("failed to strip CLI exports")?;
        }
        if self.fs.is_some() || self.stdio.is_some() {
            resolve.merge_worlds(fs_world, base_world)?;
        } else {
            strip_virt(&mut module, &["wasi:filesystem/"])
                .context("failed to strip filesystem exports")?;
        }

        let mut producers = Producers::default();
        producers.add("processed-by", "wasi-virt", env!("CARGO_PKG_VERSION"));

        component_section.data =
            metadata::encode(&resolve, base_world, StringEncoding::UTF8, Some(&producers))
                .context("failed to encode metadata")?;

        module.customs.add(component_section);

        let mut bytes = module.emit_wasm();

        // because we rely on dead code ellimination to remove unnecessary adapter code
        // we save into a temporary file and run wasm-opt before returning
        // this can be disabled with wasm_opt: false
        if self.wasm_opt.unwrap_or(true) {
            bytes = apply_wasm_opt(bytes, self.debug).context("failed to apply `wasm-opt`")?;
        }

        // now adapt the virtualized component
        let encoder = ComponentEncoder::default()
            .validate(true)
            .module(&bytes)
            .context("failed to set core component module")?;
        let encoded_bytes = encoder.encode().context("failed to encode component")?;

        let adapter = if let Some(compose_path) = &self.compose {
            let compose_path = PathBuf::from(compose_path);
            let dir = env::temp_dir();
            let tmp_virt = dir.join(format!("virt{}.wasm", timestamp()));
            fs::write(&tmp_virt, encoded_bytes).context("failed to write temporary component")?;

            let composed_bytes = ComponentComposer::new(
                &compose_path,
                &wasm_compose::config::Config {
                    definitions: vec![tmp_virt.clone()],
                    ..Default::default()
                },
            )
            .compose()
            .with_context(|| "Unable to compose virtualized adapter into component.\nMake sure virtualizations are enabled and being used.")
            .or_else(|e| {
                fs::remove_file(&tmp_virt).context("failed to remove temporary component")?;
                Err(e)
            })?;

            fs::remove_file(&tmp_virt).context("failed to remove temporary component")?;

            composed_bytes
        } else {
            encoded_bytes
        };

        Ok(VirtResult {
            adapter,
            virtual_files,
        })
    }
}

fn apply_wasm_opt(bytes: Vec<u8>, debug: bool) -> Result<Vec<u8>> {
    #[cfg(not(feature = "wasm-opt"))]
    {
        return Ok(bytes);
    }

    #[cfg(feature = "wasm-opt")]
    {
        use std::{fs, time::SystemTime};
        use wasm_opt::{Feature, OptimizationOptions, ShrinkLevel};

        fn timestamp() -> u64 {
            match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
                Ok(n) => n.as_secs(),
                Err(_) => panic!(),
            }
        }

        let dir = env::temp_dir();
        let tmp_input = dir.join(format!("virt.core.input.{}.wasm", timestamp()));
        let tmp_output = dir.join(format!("virt.core.output.{}.wasm", timestamp()));
        fs::write(&tmp_input, bytes)
            .with_context(|| "Unable to write temporary file for wasm-opt call on adapter")?;
        OptimizationOptions::new_opt_level_2()
            .shrink_level(ShrinkLevel::Level1)
            .enable_feature(Feature::All)
            .debug_info(debug)
            .run(&tmp_input, &tmp_output)
            .with_context(|| "Unable to apply wasm-opt optimization to virt. This can be disabled with wasm_opt: false.")
            .or_else(|e| {
                fs::remove_file(&tmp_input)?;
                Err(e)
            })?;
        let bytes = fs::read(&tmp_output)?;
        fs::remove_file(&tmp_input)?;
        fs::remove_file(&tmp_output)?;
        Ok(bytes)
    }
}

fn timestamp() -> u64 {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(n) => n.as_secs(),
        Err(_) => panic!(),
    }
}
