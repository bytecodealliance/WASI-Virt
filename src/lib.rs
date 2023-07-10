use anyhow::Result;
use env::{create_env_virt, VirtEnv};
use serde::Deserialize;
use wit_component::ComponentEncoder;

mod env;
mod walrus_ops;

#[derive(Deserialize, Debug, Default, Clone)]
pub struct VirtOpts {
    /// Environment virtualization
    env: Option<VirtEnv>,
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
        create_env_virt(&mut module, env)?;
    }

    let bytes = module.emit_wasm();

    // now adapt the virtualized component
    let encoder = ComponentEncoder::default().validate(true).module(&bytes)?;
    Ok(encoder.encode()?)
}
