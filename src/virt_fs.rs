use std::fmt;
use std::{collections::BTreeMap, fs};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use walrus::{ir::Value, ExportItem, GlobalKind, InitExpr, Module};

use crate::{
    data::{Data, WasmEncode},
    walrus_ops::{
        get_active_data_segment, get_stack_global, remove_exported_func, stub_imported_func,
    },
    WasiVirt,
};

pub type VirtualFiles = BTreeMap<String, String>;

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct VirtFs {
    /// Filesystem state to virtualize
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

impl WasiVirt {
    fn get_or_create_fs(&mut self) -> &mut VirtFs {
        self.virt_opts.fs.get_or_insert_with(Default::default)
    }

    pub fn preopen(mut self, name: String, preopen: FsEntry) -> Self {
        let fs = self.get_or_create_fs();
        fs.preopens.insert(name, preopen);
        self
    }

    pub fn passive_cutoff(mut self, passive_cutoff: usize) -> Self {
        let fs = self.get_or_create_fs();
        fs.passive_cutoff = Some(passive_cutoff);
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
                let path = format!("{base_path}{name}");
                sub_entry.visit_pre_mut_inner(visit, &path)?;
            }
        }
        Ok(())
    }

    pub fn visit_pre<'a, Visitor>(&'a self, base_path: &str, visit: &mut Visitor) -> Result<()>
    where
        Visitor: FnMut(&FsEntry, &str, &str, usize) -> Result<()>,
    {
        visit(self, base_path, "", 0)?;
        self.visit_pre_inner(visit, base_path)
    }

    fn visit_pre_inner<'a, Visitor>(&'a self, visit: &mut Visitor, base_path: &str) -> Result<()>
    where
        Visitor: FnMut(&FsEntry, &str, &str, usize) -> Result<()>,
    {
        match self {
            FsEntry::Dir(dir) => {
                let len = dir.iter().len();
                for (idx, (name, sub_entry)) in dir.iter().enumerate() {
                    visit(sub_entry, name, base_path, len - idx - 1)?;
                }
                for (name, sub_entry) in dir {
                    let path = format!("{base_path}{name}");
                    sub_entry.visit_pre_inner(visit, &path)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

pub fn create_fs_virt<'a>(module: &'a mut Module, fs: &VirtFs) -> Result<VirtualFiles> {
    let mut virtual_files = BTreeMap::new();

    // First we iterate the options and fill in all HostDir and HostFile entries
    // With inline directory and file entries
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
                            full_path.push('/');
                            full_path.push_str(file_name_str);
                            virtual_files.insert(format!("{path}{name}/{file_name_str}"), full_path.to_string());
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

    // Create the data section bytes
    let mut data_section = Data::new(get_stack_global(module)? as usize);
    let mut host_passthrough = false;

    // Next we linearize the pre-order directory graph as the static file data
    // Using a pre-order traversal
    // Each parent node is formed along with its child length and deep subgraph
    // length.
    let mut static_fs_data: Vec<StaticIndexEntry> = Vec::new();
    let mut preopen_indices: Vec<u32> = Vec::new();
    for (name, entry) in &fs.preopens {
        preopen_indices.push(static_fs_data.len() as u32);
        entry.visit_pre(name, &mut |entry, name, _path, remaining_siblings| {
            let name_str_ptr = data_section.string(name)?;
            let (ty, data) = match &entry {
                // removed during previous step
                FsEntry::Virtualize(_) | FsEntry::Source(_) => unreachable!(),
                FsEntry::Symlink(_) => todo!("symlink support"),
                FsEntry::RuntimeFile(path) => {
                    host_passthrough = true;
                    let str = data_section.string(path)?;
                    (
                        StaticIndexType::RuntimeHostFile,
                        StaticFileData { host_path: str },
                    )
                }
                FsEntry::RuntimeDir(path) => {
                    host_passthrough = true;
                    let str = data_section.string(path)?;
                    (
                        StaticIndexType::RuntimeHostDir,
                        StaticFileData { host_path: str },
                    )
                }
                FsEntry::Dir(dir) => {
                    let child_cnt = dir.len() as u32;
                    // children will be visited next in preorder and contiguously
                    // therefore the child index in the static fs data is known
                    // to be the next index
                    let start_idx = static_fs_data.len() as u32 + 1;
                    let child_idx = start_idx + remaining_siblings as u32;
                    (
                        StaticIndexType::Dir,
                        StaticFileData {
                            dir: (child_idx, child_cnt),
                        },
                    )
                }
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
            Ok(())
        })?;
    }

    // now write the linearized static index entry section into the data
    let static_index_addr = data_section.write_slice(static_fs_data.as_slice())?;

    let memory = module.memories.iter().nth(0).unwrap().id();

    let fs_ptr_addr = {
        let fs_ptr_export = module
            .exports
            .iter()
            .find(|expt| expt.name.as_str() == "fs")
            .context("Adapter 'fs' is not exported")?;
        let ExportItem::Global(fs_ptr_global) = fs_ptr_export.item else {
            bail!("Adapter 'fs' not a global");
        };
        let GlobalKind::Local(InitExpr::Value(Value::I32(fs_ptr_addr))) =
            &module.globals.get(fs_ptr_global).kind
        else {
            bail!("Adapter 'fs' not a local I32 global value");
        };
        *fs_ptr_addr as u32
    };

    // If host fs is disabled, remove its imports entirely
    // replacing it with a stub panic
    if !host_passthrough {
        stub_fs_virt(module)?;
    }

    let (data, data_offset) = get_active_data_segment(module, memory, fs_ptr_addr)?;

    let preopen_addr = data_section.write_slice(preopen_indices.as_slice())?;

    const FS_STATIC_LEN: usize = 16;
    if data.value.len() < data_offset + FS_STATIC_LEN {
        let padding = 4 - (data_offset + FS_STATIC_LEN) % 4;
        data.value.resize(data_offset + FS_STATIC_LEN + padding, 0);
    }

    let bytes = data.value.as_mut_slice();

    // In the existing static data segment, update the static data options.
    //
    // From virtual-adapter/src/fs.rs:
    //
    // #[repr(C)]
    // pub static mut fs: Fs = Fs {
    //     preopen_cnt: 0,                             // [byte 0]
    //     preopens: 0 as *const usize,                // [byte 4]
    //     static_index_cnt: 0,                        // [byte 8]
    //     static_index: 0 as *const StaticIndexEntry, // [byte 12]
    //     host_passthrough: false,                    // [byte 16]
    // };
    bytes[data_offset..data_offset + 4].copy_from_slice(&(fs.preopens.len() as u32).to_le_bytes());
    bytes[data_offset + 4..data_offset + 8].copy_from_slice(&(preopen_addr as u32).to_le_bytes());
    bytes[data_offset + 8..data_offset + 12]
        .copy_from_slice(&(static_fs_data.len() as u32).to_le_bytes());
    bytes[data_offset + 12..data_offset + 16]
        .copy_from_slice(&(static_index_addr as u32).to_le_bytes());
    if host_passthrough {
        bytes[data_offset + 16..data_offset + 20].copy_from_slice(&(1 as u32).to_le_bytes());
    }

    data_section.finish(module)?;

    // return the processed virtualized filesystem
    Ok(virtual_files)
}

fn stub_fs_virt(module: &mut Module) -> Result<()> {
    stub_imported_func(module, "wasi:cli-base/preopens", "get-directories", false)?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "read_via_stream",
        false,
    )?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "write_via_stream",
        false,
    )?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "append_via_stream",
        false,
    )?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "advise", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "sync-data", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "get-flags", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "get-type", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "set-size", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "set-times", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "read", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "write", false)?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "read-directory",
        false,
    )?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "sync", false)?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "create-directory-at",
        false,
    )?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "stat", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "stat-at", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "set-times-at", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "link-at", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "open-at", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "readlink-at", false)?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "remove-directory-at",
        false,
    )?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "rename-at", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "symlink-at", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "access-at", false)?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "unlink-file-at",
        false,
    )?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "change-file-permissions-at",
        false,
    )?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "change-directory-permissions-at",
        false,
    )?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "lock-shared", false)?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "lock-exclusive",
        false,
    )?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "try-lock-shared",
        false,
    )?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "try-lock-exclusive",
        false,
    )?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "unlock", false)?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "drop-descriptor",
        false,
    )?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "read-directory-entry",
        false,
    )?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "drop-directory-entry-stream",
        false,
    )?;

    stub_imported_func(
        module,
        "wasi:io/streams",
        "drop-directory-entry-stream",
        false,
    )?;
    stub_imported_func(module, "wasi:io/streams", "read", false)?;
    stub_imported_func(module, "wasi:io/streams", "blocking-read", false)?;
    stub_imported_func(module, "wasi:io/streams", "skip", false)?;
    stub_imported_func(module, "wasi:io/streams", "blocking-skip", false)?;
    stub_imported_func(
        module,
        "wasi:io/streams",
        "subscribe-to-input-stream",
        false,
    )?;
    stub_imported_func(module, "wasi:io/streams", "drop-input-stream", false)?;
    stub_imported_func(module, "wasi:io/streams", "write", false)?;
    stub_imported_func(module, "wasi:io/streams", "blocking-write", false)?;
    stub_imported_func(module, "wasi:io/streams", "write-zeroes", false)?;
    stub_imported_func(module, "wasi:io/streams", "blocking-write-zeroes", false)?;
    stub_imported_func(module, "wasi:io/streams", "splice", false)?;
    stub_imported_func(module, "wasi:io/streams", "blocking-splice", false)?;
    stub_imported_func(module, "wasi:io/streams", "forward", false)?;
    stub_imported_func(
        module,
        "wasi:io/streams",
        "subscribe-to-output-stream",
        false,
    )?;
    stub_imported_func(module, "wasi:io/streams", "drop-output-stream", false)?;
    Ok(())
}

pub(crate) fn strip_fs_virt(module: &mut Module) -> Result<()> {
    stub_fs_virt(module)?;

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
