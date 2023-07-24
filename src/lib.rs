use anyhow::{Context, Result};
use serde::Deserialize;
use std::env;
use std::fs;
use std::time::SystemTime;
use virt_env::{create_env_virt, strip_env_virt, VirtEnv};
use virt_fs::{create_fs_virt, strip_fs_virt, VirtFs};
use wasm_metadata::Producers;
use wasm_opt::Feature;
use wasm_opt::OptimizationOptions;
use wit_component::metadata;
use wit_component::ComponentEncoder;
use wit_component::StringEncoding;

mod data;
mod virt_env;
mod virt_fs;
mod walrus_ops;

pub type VirtualFiles = virt_fs::VirtualFiles;

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct VirtOpts {
    /// Environment virtualization
    pub env: Option<VirtEnv>,
    /// Filesystem virtualization
    pub fs: Option<VirtFs>,
    /// Disable wasm-opt run if desired
    pub wasm_opt: Option<bool>,
}

#[derive(Debug, Default, Clone)]
pub struct WasiVirt {
    virt_opts: VirtOpts,
}

pub struct VirtResult {
    pub adapter: Vec<u8>,
    pub virtual_files: VirtualFiles,
}

impl WasiVirt {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(&self) -> Result<VirtResult> {
        create_virt(&self.virt_opts)
    }
}

pub fn create_virt<'a>(opts: &VirtOpts) -> Result<VirtResult> {
    let virt_adapter = include_bytes!("../lib/virtual_adapter.wasm");

    let config = walrus::ModuleConfig::new();
    let mut module = config.parse(virt_adapter)?;
    module.name = Some("wasi_virt".into());

    if let Some(env) = &opts.env {
        create_env_virt(&mut module, env)?;
    } else {
        strip_env_virt(&mut module)?;
    }
    let virtual_files = if let Some(fs) = &opts.fs {
        create_fs_virt(&mut module, fs)?
    } else {
        strip_fs_virt(&mut module)?;
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
    let fs_world = bindgen.resolve.select_world(*pkg_id, Some("virtual-fs"))?;

    if opts.env.is_some() {
        bindgen.resolve.merge_worlds(env_world, base_world)?;
    }
    if opts.fs.is_some() {
        bindgen.resolve.merge_worlds(fs_world, base_world)?;
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
    if opts.wasm_opt.unwrap_or(true) {
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

fn timestamp() -> u64 {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(n) => n.as_secs(),
        Err(_) => panic!(),
    }
}
