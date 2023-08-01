use std::fmt;
use std::{collections::BTreeMap, fs};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use walrus::{ir::Value, ExportItem, GlobalKind, InitExpr, Module};

use crate::walrus_ops::remove_exported_func;
use crate::{
    data::{Data, WasmEncode},
    walrus_ops::{get_active_data_segment, get_stack_global, stub_imported_func},
};

pub type VirtualFiles = BTreeMap<String, String>;

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct VirtStdio {
    pub stdin: bool,
    pub stdout: bool,
    pub stderr: bool,
}

impl VirtStdio {
    pub fn all(&mut self, allow: bool) -> &mut Self {
        self.stdin = allow;
        self.stdout = allow;
        self.stderr = allow;
        self
    }
    pub fn stdin(&mut self, allow: bool) -> &mut Self {
        self.stdin = allow;
        self
    }
    pub fn stdout(&mut self, allow: bool) -> &mut Self {
        self.stdout = allow;
        self
    }
    pub fn stderr(&mut self, allow: bool) -> &mut Self {
        self.stderr = allow;
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
    pub fn deny_host_preopens(&mut self) {
        self.host_preopens = false;
    }
    pub fn allow_host_preopens(&mut self) {
        self.host_preopens = true;
    }
    pub fn preopen(&mut self, name: String, preopen: FsEntry) -> &mut Self {
        self.preopens.insert(name, preopen);
        self
    }
    pub fn host_preopen(&mut self, name: String, dir: String) -> &mut Self {
        self.preopens.insert(name, FsEntry::RuntimeDir(dir));
        self
    }
    pub fn virtual_preopen(&mut self, name: String, dir: String) -> &mut Self {
        self.preopens.insert(name, FsEntry::Virtualize(dir));
        self
    }
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
                let path = format!("{base_path}/{name}");
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
                    let path = format!("{base_path}/{name}");
                    sub_entry.visit_pre_inner(visit, &path)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

// io flags
const FLAGS_ENABLE_STDIN: u32 = 1 << 0;
const FLAGS_ENABLE_STDOUT: u32 = 1 << 1;
const FLAGS_ENABLE_STDERR: u32 = 1 << 2;
const FLAGS_HOST_PREOPENS: u32 = 1 << 3;
const FLAGS_HOST_PASSTHROUGH: u32 = 1 << 4;

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
    if let Some(stdio) = stdio {
        if stdio.stdin {
            flags |= FLAGS_ENABLE_STDIN;
        }
        if stdio.stdout {
            flags |= FLAGS_ENABLE_STDOUT;
        }
        if stdio.stderr {
            flags |= FLAGS_ENABLE_STDERR;
        }
        if !stdio.stdin && !stdio.stdout && !stdio.stderr {
            stub_stdio_virt(module)?;
        }
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
                                virtual_files.insert(format!("{path}{}{name}/{file_name_str}", if path.len() > 0 { "/" } else { "" }), full_path.to_string());
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

    // Next we linearize the pre-order directory graph as the static file data
    // Using a pre-order traversal
    // Each parent node is formed along with its child length and deep subgraph
    // length.
    let mut static_fs_data: Vec<StaticIndexEntry> = Vec::new();
    let mut preopen_indices: Vec<u32> = Vec::new();
    if let Some(fs) = &fs {
        for (name, entry) in &fs.preopens {
            preopen_indices.push(static_fs_data.len() as u32);
            entry.visit_pre(name, &mut |entry, name, _path, remaining_siblings| {
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
        stub_io_virt(module)?;
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

// stubs must be _comprehensive_ in order to act as full deny over entire subsystem
// when stubbing functions that are not part of the virtual adapter exports, we therefore
// have to create this functions fresh
pub(crate) fn stub_fs_virt(module: &mut Module) -> Result<()> {
    stub_imported_func(module, "wasi:cli-base/preopens", "get-directories", true)?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "read-via-stream",
        true,
    )?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "write-via-stream",
        false,
    )?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "append-via-stream",
        false,
    )?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "advise", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "sync-data", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "get-flags", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "get-type", true)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "set-size", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "set-times", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "read", true)?;
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
    stub_imported_func(module, "wasi:filesystem/filesystem", "stat", true)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "stat-at", true)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "set-times-at", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "link-at", false)?;
    stub_imported_func(module, "wasi:filesystem/filesystem", "open-at", true)?;
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
        true,
    )?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "read-directory-entry",
        true,
    )?;
    stub_imported_func(
        module,
        "wasi:filesystem/filesystem",
        "drop-directory-entry-stream",
        true,
    )?;
    Ok(())
}

pub(crate) fn stub_io_virt(module: &mut Module) -> Result<()> {
    stub_imported_func(module, "wasi:poll/poll", "drop-pollable", true)?;
    stub_imported_func(module, "wasi:poll/poll", "poll-oneoff", true)?;
    stub_imported_func(module, "wasi:io/streams", "read", false)?;
    stub_imported_func(module, "wasi:io/streams", "blocking-read", true)?;
    stub_imported_func(module, "wasi:io/streams", "skip", true)?;
    stub_imported_func(module, "wasi:io/streams", "blocking-skip", true)?;
    stub_imported_func(module, "wasi:io/streams", "subscribe-to-input-stream", true)?;
    stub_imported_func(module, "wasi:io/streams", "drop-input-stream", true)?;
    stub_imported_func(module, "wasi:io/streams", "write", true)?;
    stub_imported_func(module, "wasi:io/streams", "blocking-write", false)?;
    stub_imported_func(module, "wasi:io/streams", "write-zeroes", true)?;
    stub_imported_func(module, "wasi:io/streams", "blocking-write-zeroes", true)?;
    stub_imported_func(module, "wasi:io/streams", "splice", true)?;
    stub_imported_func(module, "wasi:io/streams", "blocking-splice", true)?;
    stub_imported_func(module, "wasi:io/streams", "forward", true)?;
    stub_imported_func(
        module,
        "wasi:io/streams",
        "subscribe-to-output-stream",
        true,
    )?;
    stub_imported_func(module, "wasi:io/streams", "drop-output-stream", true)?;
    Ok(())
}

pub(crate) fn stub_clocks_virt(module: &mut Module) -> Result<()> {
    stub_imported_func(module, "wasi:clocks/monotonic-clock", "now", true)?;
    stub_imported_func(module, "wasi:clocks/monotonic-clock", "resolution", true)?;
    stub_imported_func(module, "wasi:clocks/monotonic-clock", "subscribe", true)?;
    Ok(())
}

pub(crate) fn stub_stdio_virt(module: &mut Module) -> Result<()> {
    stub_imported_func(module, "wasi:cli-base/stdin", "get-stdin", true)?;
    stub_imported_func(module, "wasi:cli-base/stdout", "get-stdout", true)?;
    stub_imported_func(module, "wasi:cli-base/stderr", "get-stderr", true)?;
    Ok(())
}

pub(crate) fn stub_sockets_virt(module: &mut Module) -> Result<()> {
    stub_imported_func(
        module,
        "wasi:sockets/ip-name-lookup",
        "resolve-addresses",
        true,
    )?;
    stub_imported_func(
        module,
        "wasi:sockets/ip-name-lookup",
        "resolve-next-address",
        true,
    )?;
    stub_imported_func(
        module,
        "wasi:sockets/ip-name-lookup",
        "drop-resolve-address-stream",
        true,
    )?;
    stub_imported_func(module, "wasi:sockets/ip-name-lookup", "subscribe", true)?;

    stub_imported_func(module, "wasi:sockets/tcp", "start-bind", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "finish-bind", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "start-connect", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "finish-connect", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "start-listen", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "finish-listen", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "accept", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "local-address", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "remote-address", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "address-family", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "ipv6-only", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "set-ipv6-only", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "set-listen-backlog-size", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "keep-alive", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "set-keep-alive", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "no-delay", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "set-no-delay", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "unicast-hop-limit", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "set-unicast-hop-limit", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "receive-buffer-size", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "set-receive-buffer-size", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "send-buffer-size", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "set-send-buffer-size", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "subscribe", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "shutdown", true)?;
    stub_imported_func(module, "wasi:sockets/tcp", "drop-tcp-socket", true)?;

    stub_imported_func(module, "wasi:sockets/udp", "start-bind", true)?;
    stub_imported_func(module, "wasi:sockets/udp", "finish-bind", true)?;
    stub_imported_func(module, "wasi:sockets/udp", "start-connect", true)?;
    stub_imported_func(module, "wasi:sockets/udp", "finish-connect", true)?;
    stub_imported_func(module, "wasi:sockets/udp", "receive", true)?;
    stub_imported_func(module, "wasi:sockets/udp", "send", true)?;
    stub_imported_func(module, "wasi:sockets/udp", "local-address", true)?;
    stub_imported_func(module, "wasi:sockets/udp", "remote-address", true)?;
    stub_imported_func(module, "wasi:sockets/udp", "address-family", true)?;
    stub_imported_func(module, "wasi:sockets/udp", "ipv6-only", true)?;
    stub_imported_func(module, "wasi:sockets/udp", "set-ipv6-only", true)?;
    stub_imported_func(module, "wasi:sockets/udp", "unicast-hop-limit", true)?;
    stub_imported_func(module, "wasi:sockets/udp", "set-unicast-hop-limit", true)?;
    stub_imported_func(module, "wasi:sockets/udp", "receive-buffer-size", true)?;
    stub_imported_func(module, "wasi:sockets/udp", "set-receive-buffer-size", true)?;
    stub_imported_func(module, "wasi:sockets/udp", "send-buffer-size", true)?;
    stub_imported_func(module, "wasi:sockets/udp", "set-send-buffer-size", true)?;
    stub_imported_func(module, "wasi:sockets/udp", "subscribe", true)?;
    stub_imported_func(module, "wasi:sockets/udp", "drop-udp-socket", true)?;
    Ok(())
}

// strip functions only have to dce the virtual adapter
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
    Ok(())
}

pub(crate) fn strip_clocks_virt(module: &mut Module) -> Result<()> {
    stub_clocks_virt(module)?;
    remove_exported_func(module, "wasi:clocks/monotonic-clock#now")?;
    remove_exported_func(module, "wasi:clocks/monotonic-clock#resolution")?;
    remove_exported_func(module, "wasi:clocks/monotonic-clock#subscribe")?;
    Ok(())
}

pub(crate) fn strip_http_virt(module: &mut Module) -> Result<()> {
    stub_http_virt(module)?;
    remove_exported_func(module, "wasi:http/types#drop-fields")?;
    remove_exported_func(module, "wasi:http/types#new-fields")?;
    remove_exported_func(module, "wasi:http/types#fields-get")?;
    remove_exported_func(module, "wasi:http/types#fields-set")?;
    remove_exported_func(module, "wasi:http/types#fields-delete")?;
    remove_exported_func(module, "wasi:http/types#fields-append")?;
    remove_exported_func(module, "wasi:http/types#fields-entries")?;
    remove_exported_func(module, "wasi:http/types#fields-clone")?;
    remove_exported_func(module, "wasi:http/types#finish-incoming-stream")?;
    remove_exported_func(module, "wasi:http/types#finish-outgoing-stream")?;
    remove_exported_func(module, "wasi:http/types#drop-incoming-request")?;
    remove_exported_func(module, "wasi:http/types#drop-outgoing-request")?;
    remove_exported_func(module, "wasi:http/types#incoming-request-method")?;
    remove_exported_func(module, "wasi:http/types#incoming-request-path")?;
    remove_exported_func(module, "wasi:http/types#incoming-request-query")?;
    remove_exported_func(module, "wasi:http/types#incoming-request-scheme")?;
    remove_exported_func(module, "wasi:http/types#incoming-request-authority")?;
    remove_exported_func(module, "wasi:http/types#incoming-request-headers")?;
    remove_exported_func(module, "wasi:http/types#incoming-request-consume")?;
    remove_exported_func(module, "wasi:http/types#new-outgoing-request")?;
    remove_exported_func(module, "wasi:http/types#outgoing-request-write")?;
    remove_exported_func(module, "wasi:http/types#drop-response-outparam")?;
    remove_exported_func(module, "wasi:http/types#set-response-outparam")?;
    remove_exported_func(module, "wasi:http/types#drop-incoming-response")?;
    remove_exported_func(module, "wasi:http/types#drop-outgoing-response")?;
    remove_exported_func(module, "wasi:http/types#incoming-response-status")?;
    remove_exported_func(module, "wasi:http/types#incoming-response-headers")?;
    remove_exported_func(module, "wasi:http/types#incoming-response-consume")?;
    remove_exported_func(module, "wasi:http/types#new-outgoing-response")?;
    remove_exported_func(module, "wasi:http/types#outgoing-response-write")?;
    remove_exported_func(module, "wasi:http/types#drop-future-incoming-response")?;
    remove_exported_func(module, "wasi:http/types#future-incoming-response-get")?;
    remove_exported_func(module, "wasi:http/types#listen-to-future-incoming-response")?;
    Ok(())
}

pub(crate) fn stub_http_virt(module: &mut Module) -> Result<()> {
    stub_imported_func(module, "wasi:http/types", "drop-fields", false)?;
    stub_imported_func(module, "wasi:http/types", "new-fields", false)?;
    stub_imported_func(module, "wasi:http/types", "fields-get", false)?;
    stub_imported_func(module, "wasi:http/types", "fields-set", false)?;
    stub_imported_func(module, "wasi:http/types", "fields-delete", false)?;
    stub_imported_func(module, "wasi:http/types", "fields-append", false)?;
    stub_imported_func(module, "wasi:http/types", "fields-entries", false)?;
    stub_imported_func(module, "wasi:http/types", "fields-clone", false)?;
    stub_imported_func(module, "wasi:http/types", "finish-incoming-stream", false)?;
    stub_imported_func(module, "wasi:http/types", "finish-outgoing-stream", false)?;
    stub_imported_func(module, "wasi:http/types", "drop-incoming-request", false)?;
    stub_imported_func(module, "wasi:http/types", "drop-outgoing-request", false)?;
    stub_imported_func(module, "wasi:http/types", "incoming-request-method", false)?;
    stub_imported_func(module, "wasi:http/types", "incoming-request-path", false)?;
    stub_imported_func(module, "wasi:http/types", "incoming-request-query", false)?;
    stub_imported_func(module, "wasi:http/types", "incoming-request-scheme", false)?;
    stub_imported_func(
        module,
        "wasi:http/types",
        "incoming-request-authority",
        false,
    )?;
    stub_imported_func(module, "wasi:http/types", "incoming-request-headers", false)?;
    stub_imported_func(module, "wasi:http/types", "incoming-request-consume", false)?;
    stub_imported_func(module, "wasi:http/types", "new-outgoing-request", false)?;
    stub_imported_func(module, "wasi:http/types", "outgoing-request-write", false)?;
    stub_imported_func(module, "wasi:http/types", "drop-response-outparam", false)?;
    stub_imported_func(module, "wasi:http/types", "set-response-outparam", false)?;
    stub_imported_func(module, "wasi:http/types", "drop-incoming-response", false)?;
    stub_imported_func(module, "wasi:http/types", "drop-outgoing-response", false)?;
    stub_imported_func(module, "wasi:http/types", "incoming-response-status", false)?;
    stub_imported_func(
        module,
        "wasi:http/types",
        "incoming-response-headers",
        false,
    )?;
    stub_imported_func(
        module,
        "wasi:http/types",
        "incoming-response-consume",
        false,
    )?;
    stub_imported_func(module, "wasi:http/types", "new-outgoing-response", false)?;
    stub_imported_func(module, "wasi:http/types", "outgoing-response-write", false)?;
    stub_imported_func(
        module,
        "wasi:http/types",
        "drop-future-incoming-response",
        false,
    )?;
    stub_imported_func(
        module,
        "wasi:http/types",
        "future-incoming-response-get",
        false,
    )?;
    stub_imported_func(
        module,
        "wasi:http/types",
        "listen-to-future-incoming-response",
        false,
    )?;
    Ok(())
}

pub(crate) fn strip_stdio_virt(module: &mut Module) -> Result<()> {
    stub_stdio_virt(module)?;
    remove_exported_func(module, "wasi:cli-base/stdin#get-stdin")?;
    remove_exported_func(module, "wasi:cli-base/stdout#get-stdout")?;
    remove_exported_func(module, "wasi:cli-base/stderr#get-stderr")?;
    Ok(())
}

pub(crate) fn strip_io_virt(module: &mut Module) -> Result<()> {
    stub_io_virt(module)?;
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

    remove_exported_func(module, "wasi:poll/poll#drop-pollable")?;
    remove_exported_func(module, "wasi:poll/poll#poll-oneoff")?;
    Ok(())
}

pub(crate) fn strip_sockets_virt(module: &mut Module) -> Result<()> {
    remove_exported_func(module, "wasi:sockets/ip-name-lookup#resolve-addresses")?;
    remove_exported_func(module, "wasi:sockets/ip-name-lookup#resolve-next-address")?;
    remove_exported_func(
        module,
        "wasi:sockets/ip-name-lookup#drop-resolve-address-stream",
    )?;
    remove_exported_func(module, "wasi:sockets/ip-name-lookup#subscribe")?;

    remove_exported_func(module, "wasi:sockets/tcp#start-bind")?;
    remove_exported_func(module, "wasi:sockets/tcp#finish-bind")?;
    remove_exported_func(module, "wasi:sockets/tcp#start-connect")?;
    remove_exported_func(module, "wasi:sockets/tcp#finish-connect")?;
    remove_exported_func(module, "wasi:sockets/tcp#start-listen")?;
    remove_exported_func(module, "wasi:sockets/tcp#finish-listen")?;
    remove_exported_func(module, "wasi:sockets/tcp#accept")?;
    remove_exported_func(module, "wasi:sockets/tcp#local-address")?;
    remove_exported_func(module, "wasi:sockets/tcp#remote-address")?;
    remove_exported_func(module, "wasi:sockets/tcp#address-family")?;
    remove_exported_func(module, "wasi:sockets/tcp#ipv6-only")?;
    remove_exported_func(module, "wasi:sockets/tcp#set-ipv6-only")?;
    remove_exported_func(module, "wasi:sockets/tcp#set-listen-backlog-size")?;
    remove_exported_func(module, "wasi:sockets/tcp#keep-alive")?;
    remove_exported_func(module, "wasi:sockets/tcp#set-keep-alive")?;
    remove_exported_func(module, "wasi:sockets/tcp#no-delay")?;
    remove_exported_func(module, "wasi:sockets/tcp#set-no-delay")?;
    remove_exported_func(module, "wasi:sockets/tcp#unicast-hop-limit")?;
    remove_exported_func(module, "wasi:sockets/tcp#set-unicast-hop-limit")?;
    remove_exported_func(module, "wasi:sockets/tcp#receive-buffer-size")?;
    remove_exported_func(module, "wasi:sockets/tcp#set-receive-buffer-size")?;
    remove_exported_func(module, "wasi:sockets/tcp#send-buffer-size")?;
    remove_exported_func(module, "wasi:sockets/tcp#set-send-buffer-size")?;
    remove_exported_func(module, "wasi:sockets/tcp#subscribe")?;
    remove_exported_func(module, "wasi:sockets/tcp#shutdown")?;
    remove_exported_func(module, "wasi:sockets/tcp#drop-tcp-socket")?;

    remove_exported_func(module, "wasi:sockets/udp#start-bind")?;
    remove_exported_func(module, "wasi:sockets/udp#finish-bind")?;
    remove_exported_func(module, "wasi:sockets/udp#start-connect")?;
    remove_exported_func(module, "wasi:sockets/udp#finish-connect")?;
    remove_exported_func(module, "wasi:sockets/udp#receive")?;
    remove_exported_func(module, "wasi:sockets/udp#send")?;
    remove_exported_func(module, "wasi:sockets/udp#local-address")?;
    remove_exported_func(module, "wasi:sockets/udp#remote-address")?;
    remove_exported_func(module, "wasi:sockets/udp#address-family")?;
    remove_exported_func(module, "wasi:sockets/udp#ipv6-only")?;
    remove_exported_func(module, "wasi:sockets/udp#set-ipv6-only")?;
    remove_exported_func(module, "wasi:sockets/udp#unicast-hop-limit")?;
    remove_exported_func(module, "wasi:sockets/udp#set-unicast-hop-limit")?;
    remove_exported_func(module, "wasi:sockets/udp#receive-buffer-size")?;
    remove_exported_func(module, "wasi:sockets/udp#set-receive-buffer-size")?;
    remove_exported_func(module, "wasi:sockets/udp#send-buffer-size")?;
    remove_exported_func(module, "wasi:sockets/udp#set-send-buffer-size")?;
    remove_exported_func(module, "wasi:sockets/udp#subscribe")?;
    remove_exported_func(module, "wasi:sockets/udp#drop-udp-socket")?;
    Ok(())
}
