use anyhow::{Context, Result};
use serde::Deserialize;
use virt_env::{create_env_virt, strip_env_virt, VirtEnv};
use virt_fs::{create_fs_virt, strip_fs_virt, VirtFs};
use wasm_metadata::Producers;
use wit_component::metadata;
use wit_component::ComponentEncoder;
use wit_component::StringEncoding;
use wit_parser::WorldItem;

mod data;
mod virt_env;
mod virt_fs;
mod walrus_ops;

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
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
    } else {
        strip_env_virt(&mut module)?;
    }
    if let Some(fs) = &opts.fs {
        create_fs_virt(&mut module, fs)?;
    } else {
        strip_fs_virt(&mut module)?;
    }

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
    let fs_world = bindgen.resolve.select_world(*pkg_id, Some("virtual-fs"))?;

    if opts.env.is_some() {
        bindgen.resolve.merge_worlds(env_world, base_world)?;
    }
    if opts.fs.is_some() {
        bindgen.resolve.merge_worlds(fs_world, base_world)?;
    }

    // let world = &bindgen.resolve.worlds[base_world];
    // for (key, val) in &world.exports {
    //     match &val {
    //         WorldItem::Interface(iface) => {
    //             dbg!("EXPORT IFACE: {:?}", bindgen.resolve.id_of(*iface));
    //         },
    //         _ => {}
    //     };
    // }
    // for (key, val) in &world.imports {
    //     match &val {
    //         WorldItem::Interface(iface) => {
    //             dbg!("IMPORT IFACE: {:?}", bindgen.resolve.id_of(*iface));
    //         },
    //         _ => {}
    //     };
    // }

    let mut producers = Producers::default();
    producers.add("processed-by", "wasi-virt", "0.1.0");

    component_section.data = metadata::encode(
        &bindgen.resolve,
        base_world,
        StringEncoding::UTF8,
        Some(&producers),
    )?;

    module.customs.add(component_section);

    let bytes = module.emit_wasm();

    // now adapt the virtualized component
    let encoder = ComponentEncoder::default().validate(true).module(&bytes)?;
    let encoded = encoder.encode()?;

    eprintln!("HERE");

    Ok(encoded)
}
