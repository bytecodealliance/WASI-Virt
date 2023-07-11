use anyhow::Result;
use virt_env::{create_env_virt, VirtEnv};
use virt_fs::{create_fs_virt, VirtFs};
use serde::Deserialize;
use wit_component::ComponentEncoder;

mod data;
mod virt_env;
mod virt_fs;
mod walrus_ops;

#[derive(Deserialize, Debug, Default, Clone)]
pub struct VirtOpts {
    /// Environment virtualization
    env: Option<VirtEnv>,
    /// Filesystem virtualization
    fs: Option<VirtFs>,
}

#[derive(Debug, Default, Clone)]
pub struct WasiVirt {
    virt_opts: VirtOpts,
}

impl WasiVirt {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(&self) -> Result<Vec<u8>> {
        create_virt(&self.virt_opts)
    }
}

pub fn create_virt<'a>(opts: &VirtOpts) -> Result<Vec<u8>> {
    let virt_adapter = include_bytes!("../lib/virtual_adapter.wasm");

    let config = walrus::ModuleConfig::new();
    let mut module = config.parse(virt_adapter)?;

    if let Some(env) = &opts.env {
        create_env_virt(&mut module, env)?;
    }
    if let Some(fs) = &opts.fs {
        create_fs_virt(&mut module, fs)?;
    }

    let bytes = module.emit_wasm();

    // now adapt the virtualized component
    let encoder = ComponentEncoder::default().validate(true).module(&bytes)?;
    encoder.encode()
}
