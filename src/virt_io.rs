use std::{collections::BTreeMap, fmt, fs};

use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use serde::Deserialize;
use walrus::{ir::Value, ExportItem, GlobalKind, InitExpr, Module};

use crate::{
    data::{Data, WasmEncode},
    walrus_ops::{get_active_data_segment, get_stack_global, strip_virt, stub_virt},
};

pub type VirtualFiles = BTreeMap<String, String>;

#[derive(ValueEnum, Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StdioCfg {
    #[default]
    Allow,
    Ignore,
    Deny,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct VirtStdio {
    pub stdin: StdioCfg,
    pub stdout: StdioCfg,
    pub stderr: StdioCfg,
}

impl VirtStdio {
    pub fn ignore(&mut self) -> &mut Self {
        self.stdin = StdioCfg::Ignore;
        self.stdout = StdioCfg::Ignore;
        self.stderr = StdioCfg::Ignore;
        self
    }
    pub fn allow(&mut self) -> &mut Self {
        self.stdin = StdioCfg::Allow;
        self.stdout = StdioCfg::Allow;
        self.stderr = StdioCfg::Allow;
        self
    }
    pub fn deny(&mut self) -> &mut Self {
        self.stdin = StdioCfg::Deny;
        self.stdout = StdioCfg::Deny;
        self.stderr = StdioCfg::Deny;
        self
    }
    pub fn stdin(&mut self, cfg: StdioCfg) -> &mut Self {
        self.stdin = cfg;
        self
    }
    pub fn stdout(&mut self, cfg: StdioCfg) -> &mut Self {
        self.stdout = cfg;
        self
    }
    pub fn stderr(&mut self, cfg: StdioCfg) -> &mut Self {
        self.stderr = cfg;
        self
    }
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct VirtFs {
    /// Enable verbatim host preopens
    #[serde(default)]
    pub host_preopens: bool,
    /// Filesystem state to virtualize
    #[serde(default)]
    pub preopens: BTreeMap<String, FsEntry>,
    /// A cutoff size in bytes, above which
    /// files will be treated as passive segments.
    /// Per-file control may also be provided.
    pub passive_cutoff: Option<usize>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum FsEntry {
    /// symlink absolute or relative file path on the virtual filesystem
    Symlink(String),
    /// host path at virtualization time
    Virtualize(String),
    /// host path st runtime
    RuntimeDir(String),
    RuntimeFile(String),
    /// Virtual file
    File(Vec<u8>),
    /// String (UTF8) file source convenience
    Source(String),
    /// Virtual directory
    Dir(VirtDir),
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct VirtFile {
    pub bytes: Option<Vec<u8>>,
    pub source: Option<String>,
}

type VirtDir = BTreeMap<String, FsEntry>;

impl VirtFs {
    /// Deny host preopens at runtime
    pub fn deny_host_preopens(&mut self) {
        self.host_preopens = false;
    }
    /// Allow host preopens at runtime
    pub fn allow_host_preopens(&mut self) {
        self.host_preopens = true;
    }
    /// Add a preopen entry
    pub fn preopen(&mut self, name: String, preopen: FsEntry) -> &mut Self {
        self.preopens.insert(name, preopen);
        self
    }
    /// Add a runtime preopen host mapping
    pub fn host_preopen(&mut self, name: String, dir: String) -> &mut Self {
        self.preopens.insert(name, FsEntry::RuntimeDir(dir));
        self
    }
    /// Add a preopen virtualized local directory (which will be globbed)
    pub fn virtual_preopen(&mut self, name: String, dir: String) -> &mut Self {
        self.preopens.insert(name, FsEntry::Virtualize(dir));
        self
    }
    /// Set the passive cutoff size in bytes for creating Wasm passive segments
    pub fn passive_cutoff(&mut self, passive_cutoff: usize) -> &mut Self {
        self.passive_cutoff = Some(passive_cutoff);
        self
    }
}

#[derive(Debug)]
struct StaticIndexEntry {
    name: u32,
    ty: StaticIndexType,
    data: StaticFileData,
}

impl WasmEncode for StaticIndexEntry {
    fn align() -> usize {
        4
    }
    fn size() -> usize {
        16
    }
    fn encode(&self, bytes: &mut [u8]) {
        self.name.encode(&mut bytes[0..4]);
        self.ty.encode(&mut bytes[4..8]);
        self.data.encode(&mut bytes[8..16]);
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
enum StaticIndexType {
    ActiveFile,
    PassiveFile,
    Dir,
    RuntimeHostDir,
    RuntimeHostFile,
}

impl WasmEncode for StaticIndexType {
    fn align() -> usize {
        4
    }
    fn size() -> usize {
        4
    }
    fn encode(&self, bytes: &mut [u8]) {
        bytes[0..4].copy_from_slice(&(*self as u32).to_le_bytes());
    }
}

union StaticFileData {
    /// Active memory data pointer for ActiveFile
    active: (u32, u32),

    /// Passive memory element index and len for PassiveFile
    passive: (u32, u32),

    /// Host path string for HostDir / HostFile
    host_path: u32,

    /// Pointer and child entry count for Dir
    dir: (u32, u32),
}

impl WasmEncode for StaticFileData {
    fn align() -> usize {
        4
    }
    fn size() -> usize {
        8
    }
    fn encode(&self, bytes: &mut [u8]) {
        bytes[0..4].copy_from_slice(&unsafe { self.dir.0.to_le_bytes() });
        bytes[4..8].copy_from_slice(&unsafe { self.dir.1.to_le_bytes() });
    }
}

impl fmt::Debug for StaticFileData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format!(
            "STATIC [{:?}, {:?}]",
            unsafe { self.dir.0 },
            unsafe { self.dir.1 }
        ))?;
        Ok(())
    }
}

impl FsEntry {
    fn visit_pre_mut<'a, Visitor>(&'a mut self, base_path: &str, visit: &mut Visitor) -> Result<()>
    where
        Visitor: FnMut(&mut FsEntry, &str, &str) -> Result<()>,
    {
        visit(self, base_path, "")?;
        self.visit_pre_mut_inner(visit, base_path)
    }

    fn visit_pre_mut_inner<'a, Visitor>(
        &'a mut self,
        visit: &mut Visitor,
        base_path: &str,
    ) -> Result<()>
    where
        Visitor: FnMut(&mut FsEntry, &str, &str) -> Result<()>,
    {
        if let FsEntry::Dir(dir) = self {
            for (name, sub_entry) in dir.iter_mut() {
                visit(sub_entry, name, base_path)?;
            }
            for (name, sub_entry) in dir.iter_mut() {
                let path = format!(
                    "{base_path}{}{name}",
                    if base_path.ends_with('/') { "" } else { "/" }
                );
                sub_entry.visit_pre_mut_inner(visit, &path)?;
            }
        }
        Ok(())
    }

    pub fn visit_bfs<'a, Visitor>(&'a self, base_path: &str, visit: &mut Visitor) -> Result<()>
    where
        Visitor: FnMut(&FsEntry, &str, &str, usize) -> Result<()>,
    {
        visit(self, base_path, "", 1)?;
        let mut children_of = vec![(base_path.to_string(), self)];
        let mut next_children_of;
        while children_of.len() > 0 {
            next_children_of = Vec::new();
            FsEntry::visit_bfs_level(children_of, visit, &mut next_children_of)?;
            children_of = next_children_of;
        }
        Ok(())
    }

    fn visit_bfs_level<'a, Visitor>(
        children_of: Vec<(String, &'a FsEntry)>,
        visit: &mut Visitor,
        next_children_of: &mut Vec<(String, &'a FsEntry)>,
    ) -> Result<()>
    where
        Visitor: FnMut(&FsEntry, &str, &str, usize) -> Result<()>,
    {
        // first we do a full len count at this depth to be able to predict the
        // next depth offset position for children of this item from the current index
        let mut child_offset = 0;
        for (_, parent) in &children_of {
            match parent {
                FsEntry::Dir(dir) => {
                    child_offset += dir.iter().len();
                }
                _ => {}
            }
        }
        for (base_path, parent) in children_of {
            match parent {
                FsEntry::Dir(dir) => {
                    for (name, sub_entry) in dir.iter() {
                        visit(sub_entry, name, &base_path, child_offset)?;
                        child_offset -= 1;
                        let path = format!(
                            "{base_path}{}{name}",
                            if base_path.ends_with('/') { "" } else { "/" }
                        );
                        next_children_of.push((path, sub_entry));
                        if let FsEntry::Dir(dir) = sub_entry {
                            child_offset += dir.iter().len();
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

// io flags
const FLAGS_ENABLE_STDIN: u32 = 1 << 0;
const FLAGS_ENABLE_STDOUT: u32 = 1 << 1;
const FLAGS_ENABLE_STDERR: u32 = 1 << 2;
const FLAGS_IGNORE_STDIN: u32 = 1 << 3;
const FLAGS_IGNORE_STDOUT: u32 = 1 << 4;
const FLAGS_IGNORE_STDERR: u32 = 1 << 5;
const FLAGS_HOST_PREOPENS: u32 = 1 << 6;
const FLAGS_HOST_PASSTHROUGH: u32 = 1 << 7;

pub(crate) fn create_io_virt<'a>(
    module: &'a mut Module,
    fs: Option<&VirtFs>,
    stdio: Option<&VirtStdio>,
) -> Result<VirtualFiles> {
    let mut virtual_files = BTreeMap::new();
    let mut flags: u32 = 0;

    if let Some(fs) = fs {
        if fs.host_preopens {
            flags |= FLAGS_HOST_PREOPENS;
        }
    }
    let mut disable_stdio = true;
    if let Some(stdio) = stdio {
        match stdio.stdin {
            StdioCfg::Allow => {
                flags |= FLAGS_ENABLE_STDIN;
                disable_stdio = false;
            }
            StdioCfg::Ignore => flags |= FLAGS_IGNORE_STDIN,
            // deny is the default
            StdioCfg::Deny => {}
        }
        match stdio.stdout {
            StdioCfg::Allow => {
                flags |= FLAGS_ENABLE_STDOUT;
                disable_stdio = false;
            }
            StdioCfg::Ignore => flags |= FLAGS_IGNORE_STDOUT,
            StdioCfg::Deny => {}
        }
        match stdio.stderr {
            StdioCfg::Allow => {
                flags |= FLAGS_ENABLE_STDERR;
                disable_stdio = false;
            }
            StdioCfg::Ignore => flags |= FLAGS_IGNORE_STDERR,
            StdioCfg::Deny => {}
        }
    } else {
        strip_virt(module, &["wasi:cli/std", "wasi:cli/terminal"])?;
    }
    if disable_stdio {
        stub_virt(module, &["wasi:cli/std", "wasi:cli/terminal"], false)?;
    }

    // First we iterate the options and fill in all HostDir and HostFile entries
    // With inline directory and file entries
    let fs = if let Some(fs) = fs {
        let mut fs = fs.clone();
        for (name, entry) in fs.preopens.iter_mut() {
            entry.visit_pre_mut(name, &mut |entry, name, path| {
                match entry {
                    FsEntry::Source(source) => {
                        *entry = FsEntry::File(source.as_bytes().to_vec())
                    },
                    FsEntry::Virtualize(host_path) => {
                        // read a directory or file path from the host
                        let metadata = fs::metadata(&host_path)?;
                        if metadata.is_dir() {
                            let mut entries: BTreeMap<String, FsEntry> = BTreeMap::new();
                            for entry in fs::read_dir(&host_path)? {
                                let entry = entry?;
                                let file_name = entry.file_name();
                                let file_name_str = file_name.to_str().unwrap();
                                let mut full_path = host_path.clone();
                                if !full_path.ends_with('/') {
                                    full_path.push('/');
                                }
                                full_path.push_str(file_name_str);
                                virtual_files.insert(format!(
                                    "{path}{}{name}{}{file_name_str}",
                                    if path.len() > 0 && !path.ends_with('/') { "/" } else { "" },
                                    if name.len() > 0 && !name.ends_with('/') { "/" } else { "" }
                                ), full_path.to_string());
                                entries.insert(file_name_str.into(), FsEntry::Virtualize(full_path));
                            }
                            *entry = FsEntry::Dir(entries);
                        } else {
                            if !metadata.is_file() {
                                bail!("Only files and directories are currently supported for host paths to virtualize");
                            }
                            let bytes = fs::read(&host_path)?;
                            *entry = FsEntry::File(bytes)
                        }
                    }
                    FsEntry::File(_) | FsEntry::RuntimeFile(_) | FsEntry::RuntimeDir(_) | FsEntry::Symlink(_) | FsEntry::Dir(_) => {}
                }
                Ok(())
            })?;
        }
        Some(fs)
    } else {
        None
    };

    // Create the data section bytes
    let mut data_section = Data::new(get_stack_global(module)? as usize);
    let mut fs_passthrough = if let Some(fs) = &fs {
        fs.host_preopens
    } else {
        false
    };

    // Next we linearize the bfs-order directory graph as the static file data
    // Using a pre-order traversal
    // Each parent node is formed along with its child length and deep subgraph
    // length.
    let mut static_fs_data: Vec<StaticIndexEntry> = Vec::new();
    let mut preopen_indices: Vec<u32> = Vec::new();
    if let Some(fs) = &fs {
        for (name, entry) in &fs.preopens {
            preopen_indices.push(static_fs_data.len() as u32);
            let mut cur_idx = 0;
            entry.visit_bfs(name, &mut |entry, name, _path, child_offset| {
                let name_str_ptr = data_section.string(name)?;
                let (ty, data) = match &entry {
                    // removed during previous step
                    FsEntry::Virtualize(_) | FsEntry::Source(_) => unreachable!(),
                    FsEntry::Symlink(_) => todo!("symlink support"),
                    FsEntry::RuntimeFile(path) => {
                        fs_passthrough = true;
                        let str = data_section.string(path)?;
                        (
                            StaticIndexType::RuntimeHostFile,
                            StaticFileData { host_path: str },
                        )
                    }
                    FsEntry::RuntimeDir(path) => {
                        fs_passthrough = true;
                        let str = data_section.string(path)?;
                        (
                            StaticIndexType::RuntimeHostDir,
                            StaticFileData { host_path: str },
                        )
                    }
                    FsEntry::Dir(dir) => (
                        StaticIndexType::Dir,
                        StaticFileData {
                            dir: (child_offset as u32, dir.len() as u32),
                        },
                    ),
                    FsEntry::File(bytes) => {
                        let byte_len = bytes.len();
                        if byte_len > fs.passive_cutoff.unwrap_or(1024) as usize {
                            let passive_idx = data_section.passive_bytes(bytes);
                            (
                                StaticIndexType::PassiveFile,
                                StaticFileData {
                                    passive: (passive_idx, bytes.len() as u32),
                                },
                            )
                        } else {
                            let ptr = data_section.stack_bytes(bytes)?;
                            (
                                StaticIndexType::ActiveFile,
                                StaticFileData {
                                    active: (ptr, bytes.len() as u32),
                                },
                            )
                        }
                    }
                };
                static_fs_data.push(StaticIndexEntry {
                    name: name_str_ptr as u32,
                    ty,
                    data,
                });
                cur_idx += 1;
                Ok(())
            })?;
        }
    }

    // now write the linearized static index entry section into the data
    let static_index_addr = data_section.write_slice(static_fs_data.as_slice())?;

    let memory = module.memories.iter().nth(0).unwrap().id();

    let io_ptr_addr = {
        let io_ptr_export = module
            .exports
            .iter()
            .find(|expt| expt.name.as_str() == "io")
            .context("Virt adapter 'io' is not exported")?;
        let ExportItem::Global(io_ptr_global) = io_ptr_export.item else {
            bail!("Virt adapter 'io' not a global");
        };
        let GlobalKind::Local(InitExpr::Value(Value::I32(io_ptr_addr))) =
            &module.globals.get(io_ptr_global).kind
        else {
            bail!("Virt adapter 'io' not a local I32 global value");
        };
        *io_ptr_addr as u32
    };

    // If host fs is disabled, remove its imports entirely
    // replacing it with a stub panic
    if !fs_passthrough {
        if disable_stdio {
            stub_virt(module, &["wasi:io/"], false)?;
        }
        stub_virt(module, &["wasi:filesystem/"], false)?;
    } else {
        flags |= FLAGS_HOST_PASSTHROUGH;
    }

    let (data, data_offset) = get_active_data_segment(module, memory, io_ptr_addr)?;

    let preopen_addr = data_section.write_slice(preopen_indices.as_slice())?;

    const FS_STATIC_LEN: usize = 16;
    if data.value.len() < data_offset + FS_STATIC_LEN {
        let padding = 4 - (data_offset + FS_STATIC_LEN) % 4;
        data.value.resize(data_offset + FS_STATIC_LEN + padding, 0);
    }

    let bytes = data.value.as_mut_slice();

    // In the existing static data segment, update the static data options.
    //
    // From virtual-adapter/src/io.rs:
    //
    // #[repr(C)]
    // pub static mut io: Io = Io {
    //     preopen_cnt: 0,                             // [byte 0]
    //     preopens: 0 as *const usize,                // [byte 4]
    //     static_index_cnt: 0,                        // [byte 8]
    //     static_index: 0 as *const StaticIndexEntry, // [byte 12]
    //     flags: 0                                    // [byte 16]
    // };
    if let Some(fs) = &fs {
        bytes[data_offset..data_offset + 4]
            .copy_from_slice(&(fs.preopens.len() as u32).to_le_bytes());
    }
    bytes[data_offset + 4..data_offset + 8].copy_from_slice(&(preopen_addr as u32).to_le_bytes());
    bytes[data_offset + 8..data_offset + 12]
        .copy_from_slice(&(static_fs_data.len() as u32).to_le_bytes());
    bytes[data_offset + 12..data_offset + 16]
        .copy_from_slice(&(static_index_addr as u32).to_le_bytes());

    bytes[data_offset + 16..data_offset + 20].copy_from_slice(&flags.to_le_bytes());

    data_section.finish(module)?;

    // return the processed virtualized filesystem
    Ok(virtual_files)
}
