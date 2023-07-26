use anyhow::{Context, Result};
use serde::Deserialize;
use std::env;
use std::fs;
use std::time::SystemTime;
use virt_env::{create_env_virt, strip_env_virt};
use virt_io::create_io_virt;
use virt_io::stub_io_virt;
use virt_io::VirtStdio;
use walrus::Module;
use walrus::ValType;
use walrus_ops::add_stub_exported_func;
use walrus_ops::remove_exported_func;
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

        let mut has_io = false;

        if let Some(env) = &self.env {
            create_env_virt(&mut module, env)?;
        } else {
            strip_env_virt(&mut module)?;
        }

        let virtual_files = if self.fs.is_some() || self.stdio.is_some() {
            has_io = true;
            // pull in one io subsystem -> pull in all io subsystems
            // (due to virtualization wrapping required for streams + poll)
            self.fs();
            self.stdio();
            create_io_virt(
                &mut module,
                self.fs.as_ref().unwrap(),
                self.stdio.as_ref().unwrap(),
            )?
        } else {
            Default::default()
        };

        if matches!(self.exit, Some(VirtExit::Unreachable)) {
            add_stub_exported_func(
                &mut module,
                "wasi:cli-base/exit#exit",
                vec![ValType::I32],
                vec![],
            )?;
        }

        if !has_io {
            strip_io_virt(&mut module)?;
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
        let io_world = bindgen.resolve.select_world(*pkg_id, Some("virtual-io"))?;

        if self.env.is_some() {
            bindgen.resolve.merge_worlds(env_world, base_world)?;
        }
        if has_io {
            bindgen.resolve.merge_worlds(io_world, base_world)?;
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

fn strip_io_virt(module: &mut Module) -> Result<()> {
    stub_io_virt(module)?;

    remove_exported_func(module, "wasi:cli-base/preopens#get-directories")?;

    remove_exported_func(module, "wasi:filesystem/filesystem#read-via-stream")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#write-via-stream")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#append-via-stream")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#advise")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#sync-data")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#get-flags")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#get-type")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#set-size")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#set-times")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#read")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#write")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#read-directory")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#sync")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#create-directory-at")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#stat")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#stat-at")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#set-times-at")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#link-at")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#open-at")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#readlink-at")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#remove-directory-at")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#rename-at")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#symlink-at")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#access-at")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#unlink-file-at")?;
    remove_exported_func(
        module,
        "wasi:filesystem/filesystem#change-file-permissions-at",
    )?;
    remove_exported_func(
        module,
        "wasi:filesystem/filesystem#change-directory-permissions-at",
    )?;
    remove_exported_func(module, "wasi:filesystem/filesystem#lock-shared")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#lock-exclusive")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#try-lock-shared")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#try-lock-exclusive")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#unlock")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#drop-descriptor")?;
    remove_exported_func(module, "wasi:filesystem/filesystem#read-directory-entry")?;
    remove_exported_func(
        module,
        "wasi:filesystem/filesystem#drop-directory-entry-stream",
    )?;

    remove_exported_func(module, "wasi:io/streams#read")?;
    remove_exported_func(module, "wasi:io/streams#blocking-read")?;
    remove_exported_func(module, "wasi:io/streams#skip")?;
    remove_exported_func(module, "wasi:io/streams#blocking-skip")?;
    remove_exported_func(module, "wasi:io/streams#subscribe-to-input-stream")?;
    remove_exported_func(module, "wasi:io/streams#drop-input-stream")?;
    remove_exported_func(module, "wasi:io/streams#write")?;
    remove_exported_func(module, "wasi:io/streams#blocking-write")?;
    remove_exported_func(module, "wasi:io/streams#write-zeroes")?;
    remove_exported_func(module, "wasi:io/streams#blocking-write-zeroes")?;
    remove_exported_func(module, "wasi:io/streams#splice")?;
    remove_exported_func(module, "wasi:io/streams#blocking-splice")?;
    remove_exported_func(module, "wasi:io/streams#forward")?;
    remove_exported_func(module, "wasi:io/streams#subscribe-to-output-stream")?;
    remove_exported_func(module, "wasi:io/streams#drop-output-stream")?;

    Ok(())
}
