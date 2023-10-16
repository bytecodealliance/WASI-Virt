use crate::exports::wasi::cli::stderr::Guest as Stderr;
use crate::exports::wasi::cli::stdin::Guest as Stdin;
use crate::exports::wasi::cli::stdout::Guest as Stdout;
use crate::exports::wasi::cli::terminal_input::Guest as TerminalInput;
use crate::exports::wasi::cli::terminal_output::Guest as TerminalOutput;
use crate::exports::wasi::cli::terminal_stderr::Guest as TerminalStderr;
use crate::exports::wasi::cli::terminal_stdin::Guest as TerminalStdin;
use crate::exports::wasi::cli::terminal_stdout::Guest as TerminalStdout;
use crate::exports::wasi::clocks::monotonic_clock::Guest as MonotonicClock;
use crate::exports::wasi::filesystem::preopens::Guest as Preopens;
use crate::exports::wasi::filesystem::types::{
    AccessType, Advice, Datetime, DescriptorFlags, DescriptorStat, DescriptorType, DirectoryEntry,
    ErrorCode, Guest as FilesystemTypes, MetadataHashValue, Modes, NewTimestamp, OpenFlags,
    PathFlags,
};
use crate::exports::wasi::http::types::{
    Error, Fields, Guest as HttpTypes, Headers, Method, Scheme, StatusCode, Trailers,
};
use crate::exports::wasi::io::streams::{
    Guest as Streams, InputStream, OutputStream, StreamStatus, WriteError,
};
use crate::exports::wasi::poll::poll::Guest as Poll;
use crate::exports::wasi::sockets::ip_name_lookup::{
    Guest as IpNameLookup, IpAddress, IpAddressFamily, Network, ResolveAddressStream,
};
use crate::exports::wasi::sockets::tcp::ErrorCode as NetworkErrorCode;
use crate::exports::wasi::sockets::tcp::{Guest as Tcp, IpSocketAddress, ShutdownType, TcpSocket};
use crate::exports::wasi::sockets::udp::{Datagram, Guest as Udp, UdpSocket};

use crate::wasi::cli::stderr;
use crate::wasi::cli::stdin;
use crate::wasi::cli::stdout;
// use crate::wasi::cli::terminal_input;
// use crate::wasi::cli::terminal_output;
// use crate::wasi::cli::terminal_stderr;
// use crate::wasi::cli::terminal_stdin;
// use crate::wasi::cli::terminal_stdout;
use crate::wasi::filesystem::preopens;
use crate::wasi::filesystem::types as filesystem_types;
use crate::wasi::io::streams;

// these are all the subsystems which touch streams + poll
use crate::wasi::clocks::monotonic_clock;
use crate::wasi::http::types as http_types;
use crate::wasi::poll::poll;
use crate::wasi::sockets::ip_name_lookup;
use crate::wasi::sockets::tcp;
use crate::wasi::sockets::udp;

use crate::VirtAdapter;

// for debugging build
const DEBUG: bool = cfg!(feature = "debug");

use std::alloc::Layout;
use std::cmp;
use std::collections::BTreeMap;
use std::ffi::CStr;
use std::slice;

// io flags
const FLAGS_ENABLE_STDIN: u32 = 1 << 0;
const FLAGS_ENABLE_STDOUT: u32 = 1 << 1;
const FLAGS_ENABLE_STDERR: u32 = 1 << 2;
const FLAGS_IGNORE_STDIN: u32 = 1 << 3;
const FLAGS_IGNORE_STDOUT: u32 = 1 << 4;
const FLAGS_IGNORE_STDERR: u32 = 1 << 5;
const FLAGS_HOST_PREOPENS: u32 = 1 << 6;
const FLAGS_HOST_PASSTHROUGH: u32 = 1 << 7;

#[macro_export]
macro_rules! debug {
    ($dst:expr, $($arg:tt)*) => {
        if DEBUG {
            log(&format!($dst, $($arg)*));
        }
    };
    ($dst:expr) => {
        if DEBUG {
            log($dst);
        }
    };
}

fn log(msg: &str) {
    if let Some(stderr) = unsafe { STATE.host_stderr } {
        let msg = format!("{}\n", msg);
        let _ = streams::blocking_write_and_flush(stderr, msg.as_bytes());
    }
}

// static fs config
#[repr(C)]
pub struct Io {
    preopen_cnt: usize,
    preopens: *const usize,
    static_index_cnt: usize,
    static_index: *const StaticIndexEntry,
    flags: u32,
}

enum AllowCfg {
    Allow,
    Deny,
    Ignore,
}

impl Io {
    fn preopens() -> Vec<&'static StaticIndexEntry> {
        let preopen_offsets = unsafe { slice::from_raw_parts(io.preopens, io.preopen_cnt) };
        let static_index = Io::static_index();
        preopen_offsets
            .iter()
            .map(|&idx| &static_index[idx])
            .collect()
    }
    fn static_index() -> &'static [StaticIndexEntry] {
        unsafe { slice::from_raw_parts(io.static_index, io.static_index_cnt) }
    }
    fn stdin() -> AllowCfg {
        if (unsafe { io.flags }) & FLAGS_ENABLE_STDIN > 0 {
            AllowCfg::Allow
        } else if (unsafe { io.flags }) & FLAGS_IGNORE_STDIN > 0 {
            AllowCfg::Ignore
        } else {
            AllowCfg::Deny
        }
    }
    fn stdout() -> AllowCfg {
        if (unsafe { io.flags }) & FLAGS_ENABLE_STDOUT > 0 {
            AllowCfg::Allow
        } else if (unsafe { io.flags }) & FLAGS_IGNORE_STDOUT > 0 {
            AllowCfg::Ignore
        } else {
            AllowCfg::Deny
        }
    }
    fn stderr() -> AllowCfg {
        if (unsafe { io.flags }) & FLAGS_ENABLE_STDERR > 0 {
            AllowCfg::Allow
        } else if (unsafe { io.flags }) & FLAGS_IGNORE_STDERR > 0 {
            AllowCfg::Ignore
        } else {
            AllowCfg::Deny
        }
    }
    fn host_passthrough() -> bool {
        (unsafe { io.flags }) & FLAGS_HOST_PASSTHROUGH > 0
    }
    fn host_preopens() -> bool {
        (unsafe { io.flags }) & FLAGS_HOST_PREOPENS > 0
    }
}

#[derive(Debug)]
enum DescriptorTarget {
    StaticEntry(*const StaticIndexEntry),
    HostDescriptor(u32),
}

#[derive(Debug)]
struct Descriptor {
    // the descriptor index of this descriptor
    fd: u32,
    target: DescriptorTarget,
}

impl Descriptor {
    fn drop(&self) {
        unsafe {
            STATE.descriptor_table.remove(&self.fd);
        }
        if let DescriptorTarget::HostDescriptor(host_fd) = self.target {
            filesystem_types::drop_descriptor(host_fd);
        }
    }

    fn get_type(&self) -> Result<DescriptorType, ErrorCode> {
        match self.target {
            DescriptorTarget::StaticEntry(ptr) => {
                let entry = entry(ptr);
                Ok(entry.ty())
            }
            DescriptorTarget::HostDescriptor(host_fd) => filesystem_types::get_type(host_fd)
                .map(descriptor_ty_map)
                .map_err(err_map),
        }
    }

    fn read<'a>(&mut self, offset: u64, len: u64) -> Result<(Vec<u8>, bool), ErrorCode> {
        match self.target {
            DescriptorTarget::StaticEntry(ptr) => {
                let entry = entry(ptr);
                match entry.ty {
                    StaticIndexType::ActiveFile => {
                        if offset as usize == unsafe { entry.data.active.1 } {
                            return Ok((vec![], true));
                        }
                        if offset as usize > unsafe { entry.data.active.1 } {
                            return Err(ErrorCode::InvalidSeek);
                        }
                        let read_ptr = unsafe { entry.data.active.0.add(offset as usize) };
                        let read_len = cmp::min(
                            unsafe { entry.data.active.1 } - offset as usize,
                            len as usize,
                        );
                        let bytes = unsafe { slice::from_raw_parts(read_ptr, read_len) };
                        Ok((bytes.to_vec(), read_len < len as usize))
                    }
                    StaticIndexType::PassiveFile => {
                        if offset as usize == unsafe { entry.data.passive.1 } {
                            return Ok((vec![], true));
                        }
                        if offset as usize > unsafe { entry.data.passive.1 } {
                            return Err(ErrorCode::InvalidSeek);
                        }
                        let read_len = cmp::min(
                            unsafe { entry.data.passive.1 } - offset as usize,
                            len as usize,
                        );
                        let data = passive_alloc(
                            unsafe { entry.data.passive.0 },
                            offset as u32,
                            read_len as u32,
                        );
                        let bytes = unsafe { slice::from_raw_parts(data, read_len) };
                        let vec = bytes.to_vec();
                        unsafe {
                            std::alloc::dealloc(data, Layout::from_size_align(1, 4).unwrap())
                        };
                        Ok((vec, read_len < len as usize))
                    }
                    StaticIndexType::RuntimeDir | StaticIndexType::Dir => {
                        Err(ErrorCode::IsDirectory)
                    }
                    StaticIndexType::RuntimeFile => {
                        // log("Internal error: Runtime file should not be reflected directly on descriptors");
                        unreachable!();
                    }
                }
            }
            DescriptorTarget::HostDescriptor(host_fd) => {
                filesystem_types::read(host_fd, len, offset).map_err(err_map)
            }
        }
    }
}

fn dir_map(d: filesystem_types::DirectoryEntry) -> DirectoryEntry {
    DirectoryEntry {
        type_: descriptor_ty_map(d.type_),
        name: d.name,
    }
}

fn stat_map(s: filesystem_types::DescriptorStat) -> DescriptorStat {
    DescriptorStat {
        type_: descriptor_ty_map(s.type_),
        link_count: s.link_count,
        size: s.size,
        data_access_timestamp: s.data_modification_timestamp,
        data_modification_timestamp: s.data_modification_timestamp,
        status_change_timestamp: s.status_change_timestamp,
    }
}

fn descriptor_ty_map(d: filesystem_types::DescriptorType) -> DescriptorType {
    match d {
        filesystem_types::DescriptorType::Unknown => DescriptorType::Unknown,
        filesystem_types::DescriptorType::BlockDevice => DescriptorType::BlockDevice,
        filesystem_types::DescriptorType::CharacterDevice => DescriptorType::CharacterDevice,
        filesystem_types::DescriptorType::Directory => DescriptorType::Directory,
        filesystem_types::DescriptorType::Fifo => DescriptorType::Fifo,
        filesystem_types::DescriptorType::SymbolicLink => DescriptorType::SymbolicLink,
        filesystem_types::DescriptorType::RegularFile => DescriptorType::RegularFile,
        filesystem_types::DescriptorType::Socket => DescriptorType::Socket,
    }
}

fn err_map(e: filesystem_types::ErrorCode) -> ErrorCode {
    match e {
        filesystem_types::ErrorCode::Access => ErrorCode::Access,
        filesystem_types::ErrorCode::WouldBlock => ErrorCode::WouldBlock,
        filesystem_types::ErrorCode::Already => ErrorCode::Already,
        filesystem_types::ErrorCode::BadDescriptor => ErrorCode::BadDescriptor,
        filesystem_types::ErrorCode::Busy => ErrorCode::Busy,
        filesystem_types::ErrorCode::Deadlock => ErrorCode::Deadlock,
        filesystem_types::ErrorCode::Quota => ErrorCode::Quota,
        filesystem_types::ErrorCode::Exist => ErrorCode::Exist,
        filesystem_types::ErrorCode::FileTooLarge => ErrorCode::FileTooLarge,
        filesystem_types::ErrorCode::IllegalByteSequence => ErrorCode::IllegalByteSequence,
        filesystem_types::ErrorCode::InProgress => ErrorCode::InProgress,
        filesystem_types::ErrorCode::Interrupted => ErrorCode::Interrupted,
        filesystem_types::ErrorCode::Invalid => ErrorCode::Invalid,
        filesystem_types::ErrorCode::Io => ErrorCode::Io,
        filesystem_types::ErrorCode::IsDirectory => ErrorCode::IsDirectory,
        filesystem_types::ErrorCode::Loop => ErrorCode::Loop,
        filesystem_types::ErrorCode::TooManyLinks => ErrorCode::TooManyLinks,
        filesystem_types::ErrorCode::MessageSize => ErrorCode::MessageSize,
        filesystem_types::ErrorCode::NameTooLong => ErrorCode::NameTooLong,
        filesystem_types::ErrorCode::NoDevice => ErrorCode::NoDevice,
        filesystem_types::ErrorCode::NoEntry => ErrorCode::NoEntry,
        filesystem_types::ErrorCode::NoLock => ErrorCode::NoLock,
        filesystem_types::ErrorCode::InsufficientMemory => ErrorCode::InsufficientMemory,
        filesystem_types::ErrorCode::InsufficientSpace => ErrorCode::InsufficientSpace,
        filesystem_types::ErrorCode::NotDirectory => ErrorCode::NotDirectory,
        filesystem_types::ErrorCode::NotEmpty => ErrorCode::NotEmpty,
        filesystem_types::ErrorCode::NotRecoverable => ErrorCode::NotRecoverable,
        filesystem_types::ErrorCode::Unsupported => ErrorCode::Unsupported,
        filesystem_types::ErrorCode::NoTty => ErrorCode::NoTty,
        filesystem_types::ErrorCode::NoSuchDevice => ErrorCode::NoSuchDevice,
        filesystem_types::ErrorCode::Overflow => ErrorCode::Overflow,
        filesystem_types::ErrorCode::NotPermitted => ErrorCode::NotPermitted,
        filesystem_types::ErrorCode::Pipe => ErrorCode::Pipe,
        filesystem_types::ErrorCode::ReadOnly => ErrorCode::ReadOnly,
        filesystem_types::ErrorCode::InvalidSeek => ErrorCode::InvalidSeek,
        filesystem_types::ErrorCode::TextFileBusy => ErrorCode::TextFileBusy,
        filesystem_types::ErrorCode::CrossDevice => ErrorCode::CrossDevice,
    }
}

fn entry(ptr: *const StaticIndexEntry) -> &'static StaticIndexEntry {
    unsafe { ptr.as_ref() }.unwrap()
}

impl StaticIndexEntry {
    fn idx(&self) -> usize {
        let static_index_start = unsafe { io.static_index };
        let cur_index_start = self as *const StaticIndexEntry;
        unsafe { cur_index_start.sub_ptr(static_index_start) }
    }
    fn runtime_path(&self) -> &'static str {
        let c_str = unsafe { CStr::from_ptr((*self).data.runtime_path) };
        c_str.to_str().unwrap()
    }
    fn name(&self) -> &'static str {
        let c_str = unsafe { CStr::from_ptr((*self).name) };
        c_str.to_str().unwrap()
    }
    fn ty(&self) -> DescriptorType {
        match self.ty {
            StaticIndexType::ActiveFile
            | StaticIndexType::PassiveFile
            | StaticIndexType::RuntimeFile => DescriptorType::RegularFile,
            StaticIndexType::Dir | StaticIndexType::RuntimeDir => DescriptorType::Directory,
        }
    }
    fn size(&self) -> Result<u64, ErrorCode> {
        match self.ty {
            StaticIndexType::ActiveFile => Ok(unsafe { self.data.active.1 } as u64),
            StaticIndexType::PassiveFile => Ok(unsafe { self.data.passive.1 } as u64),
            StaticIndexType::Dir | StaticIndexType::RuntimeDir => Ok(0),
            StaticIndexType::RuntimeFile => {
                let Some((fd, subpath)) = IoState::get_host_preopen(self.runtime_path()) else {
                    return Err(ErrorCode::NoEntry);
                };
                let stat =
                    filesystem_types::stat_at(fd, filesystem_types::PathFlags::empty(), subpath)
                        .map_err(err_map)?;
                Ok(stat.size)
            }
        }
    }
    fn child_list(&self) -> Result<&'static [StaticIndexEntry], ErrorCode> {
        if !matches!(self.ty(), DescriptorType::Directory) {
            return Err(ErrorCode::NotDirectory);
        }
        let (child_offset, child_list_len) = unsafe { (*self).data.dir };
        let static_index = Io::static_index();
        Ok(&static_index[self.idx() + child_offset..self.idx() + child_offset + child_list_len])
    }
    fn dir_lookup(&self, path: &str) -> Result<&'static StaticIndexEntry, ErrorCode> {
        assert!(path.len() > 0);
        let (first_part, rem) = match path.find('/') {
            Some(idx) => (&path[0..idx], &path[idx + 1..]),
            None => (path, ""),
        };
        let child_list = self.child_list()?;
        if let Ok(child_idx) = child_list.binary_search_by(|entry| entry.name().cmp(first_part)) {
            let child = &child_list[child_idx];
            if rem.len() > 0 {
                child.dir_lookup(rem)
            } else {
                Ok(child)
            }
        } else {
            Err(ErrorCode::NoEntry)
        }
    }
}

// #[derive(Debug)]
#[repr(C)]
struct StaticIndexEntry {
    name: *const i8,
    ty: StaticIndexType,
    data: StaticFileData,
}

#[repr(C)]
union StaticFileData {
    /// Active memory data pointer for ActiveFile
    active: (*const u8, usize),
    /// Passive memory element index and len for PassiveFile
    passive: (u32, usize),
    /// Host path string for HostDir / HostFile
    runtime_path: *const i8,
    // Index and child entry count for Dir
    dir: (usize, usize),
}

// impl fmt::Debug for StaticFileData {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         f.write_str(&format!(
//             "STATIC [{:?}, {:?}]",
//             unsafe { self.dir.0 },
//             unsafe { self.dir.1 }
//         ))?;
//         Ok(())
//     }
// }

#[derive(Debug)]
#[allow(dead_code)]
#[repr(u32)]
enum StaticIndexType {
    ActiveFile,
    PassiveFile,
    Dir,
    RuntimeDir,
    RuntimeFile,
}

// This function gets mutated by the virtualizer
#[no_mangle]
#[inline(never)]
pub fn passive_alloc(passive_idx: u32, offset: u32, len: u32) -> *mut u8 {
    return (passive_idx + offset + len) as *mut u8;
}

#[no_mangle]
pub static mut io: Io = Io {
    preopen_cnt: 0,                             // [byte 0]
    preopens: 0 as *const usize,                // [byte 4]
    static_index_cnt: 0,                        // [byte 8]
    static_index: 0 as *const StaticIndexEntry, // [byte 12]
    flags: 0,                                   // [byte 16]
};

// local fs state
pub struct IoState {
    initialized: bool,
    descriptor_cnt: u32,
    preopen_directories: Vec<(u32, String)>,
    host_preopen_directories: BTreeMap<String, u32>,
    descriptor_table: BTreeMap<u32, Descriptor>,
    host_stderr: Option<u32>,
    stream_cnt: u32,
    stream_table: BTreeMap<u32, Stream>,
    poll_cnt: u32,
    poll_table: BTreeMap<u32, PollTarget>,
}

enum PollTarget {
    Null,
    Host(u32),
}

static mut STATE: IoState = IoState {
    initialized: false,
    descriptor_cnt: 0,
    preopen_directories: Vec::new(),
    host_preopen_directories: BTreeMap::new(),
    descriptor_table: BTreeMap::new(),
    host_stderr: None,
    stream_cnt: 0,
    stream_table: BTreeMap::new(),
    poll_cnt: 0,
    poll_table: BTreeMap::new(),
};

enum Stream {
    // null stream
    Null,
    // error stream
    Err,
    StaticFile(StaticFileStream),
    StaticDir(StaticDirStream),
    Host(u32),
}

impl From<StaticFileStream> for Stream {
    fn from(value: StaticFileStream) -> Self {
        Stream::StaticFile(value)
    }
}

impl From<StaticDirStream> for Stream {
    fn from(value: StaticDirStream) -> Self {
        Stream::StaticDir(value)
    }
}

struct StaticFileStream {
    // local file descriptor
    fd: u32,
    // current offset
    offset: u64,
}

struct StaticDirStream {
    fd: u32,
    idx: usize,
}

impl StaticFileStream {
    fn new(fd: u32) -> Self {
        Self { fd, offset: 0 }
    }
    fn read(&mut self, len: u64) -> Result<(Vec<u8>, StreamStatus), ()> {
        let descriptor = IoState::get_descriptor(self.fd).map_err(|_| ())?;
        let (bytes, done) = descriptor.read(self.offset, len).map_err(|_| ())?;
        self.offset += bytes.len() as u64;
        Ok((
            bytes,
            if done {
                StreamStatus::Ended
            } else {
                StreamStatus::Open
            },
        ))
    }
}

impl StaticDirStream {
    fn new(fd: u32) -> Self {
        Self { fd, idx: 0 }
    }
    fn next(&mut self) -> Result<Option<DirectoryEntry>, ErrorCode> {
        let descriptor = IoState::get_descriptor(self.fd)?;
        let DescriptorTarget::StaticEntry(ptr) = descriptor.target else {
            unreachable!()
        };
        let entry = entry(ptr);
        let child_list = entry.child_list()?;
        let child = if self.idx < child_list.len() {
            let child = &child_list[self.idx];
            Some(DirectoryEntry {
                type_: child.ty(),
                name: child.name().into(),
            })
        } else {
            None
        };
        self.idx += 1;
        Ok(child)
    }
}

impl IoState {
    fn initialize() {
        if unsafe { STATE.initialized } {
            return;
        }
        // the first three streams are always stdin, stdout, stderr
        assert!(unsafe { STATE.stream_cnt } == 0);
        IoState::new_stream(match Io::stdin() {
            AllowCfg::Allow => Stream::Host(stdin::get_stdin()),
            AllowCfg::Ignore => Stream::Null,
            AllowCfg::Deny => Stream::Err,
        });
        IoState::new_stream(match Io::stdout() {
            AllowCfg::Allow => Stream::Host(stdout::get_stdout()),
            AllowCfg::Ignore => Stream::Null,
            AllowCfg::Deny => Stream::Err,
        });
        IoState::new_stream(match Io::stderr() {
            AllowCfg::Allow => {
                let stderr = stderr::get_stderr();
                unsafe { STATE.host_stderr = Some(stderr) };
                Stream::Host(stderr)
            }
            AllowCfg::Ignore => Stream::Null,
            AllowCfg::Deny => Stream::Err,
        });
        assert!(unsafe { STATE.stream_cnt } == 3);

        if Io::host_passthrough() || Io::host_preopens() {
            let host_preopen_directories = unsafe { &mut STATE.host_preopen_directories };
            for (fd, name) in preopens::get_directories() {
                if Io::host_preopens() {
                    let fd = IoState::new_descriptor(DescriptorTarget::HostDescriptor(fd));
                    let entry = (fd, name.to_string());
                    unsafe { STATE.preopen_directories.push(entry) }
                }
                if Io::host_passthrough() {
                    host_preopen_directories.insert(name, fd);
                }
            }
        }

        let preopens = Io::preopens();
        for preopen in preopens {
            let fd = IoState::new_descriptor(DescriptorTarget::StaticEntry(preopen));
            let entry = (fd, preopen.name().to_string());
            unsafe { STATE.preopen_directories.push(entry) }
        }

        // we have one virtual pollable at poll 0 which is a null pollable
        // this is just an immediately resolving pollable
        unsafe { STATE.poll_cnt += 1 };
        unsafe { STATE.poll_table.insert(0, PollTarget::Null) };

        unsafe { STATE.initialized = true };
    }
    fn get_host_preopen<'a>(path: &'a str) -> Option<(u32, &'a str)> {
        let path = if path.starts_with("./") {
            &path[2..]
        } else {
            path
        };
        for (preopen_name, fd) in unsafe { &STATE.host_preopen_directories } {
            let preopen_name = if preopen_name.starts_with("./") {
                &preopen_name[2..]
            } else if preopen_name.starts_with(".") {
                &preopen_name[1..]
            } else {
                preopen_name
            };
            if path.starts_with(preopen_name) {
                // ambient relative
                if preopen_name.len() == 0 {
                    if path.as_bytes()[0] != b'/' {
                        return Some((*fd, &path));
                    }
                } else {
                    // root '/' match
                    if preopen_name == "/" && path.as_bytes()[0] == b'/' {
                        return Some((*fd, &path[1..]));
                    }
                    // exact match
                    if preopen_name.len() == path.len() {
                        return Some((*fd, ""));
                    }
                    // normal [x]/ match
                    if path.as_bytes()[preopen_name.len()] == b'/' {
                        return Some((*fd, &path[preopen_name.len() + 1..]));
                    }
                }
            }
        }
        None
    }
    fn new_descriptor(target: DescriptorTarget) -> u32 {
        let fd = unsafe { STATE.descriptor_cnt };
        let descriptor = Descriptor { fd, target };
        assert!(unsafe { STATE.descriptor_table.insert(fd, descriptor) }.is_none());
        unsafe { STATE.descriptor_cnt += 1 };
        fd
    }
    fn get_descriptor<'a>(fd: u32) -> Result<&'a mut Descriptor, ErrorCode> {
        match unsafe { STATE.descriptor_table.get_mut(&fd) } {
            Some(descriptor) => Ok(descriptor),
            None => Err(ErrorCode::BadDescriptor),
        }
    }
    fn new_stream<S: Into<Stream>>(stream: S) -> u32 {
        let sid = unsafe { STATE.stream_cnt };
        unsafe { STATE.stream_cnt += 1 };
        unsafe { STATE.stream_table.insert(sid, stream.into()) };
        sid
    }
    fn get_stream<'a>(sid: u32) -> Result<&'a mut Stream, ()> {
        match unsafe { STATE.stream_table.get_mut(&sid) } {
            Some(stream) => Ok(stream),
            None => Err(()),
        }
    }
    fn new_poll(target: PollTarget) -> u32 {
        let pid = unsafe { STATE.poll_cnt };
        unsafe { STATE.poll_cnt += 1 };
        unsafe { STATE.poll_table.insert(pid, target) };
        pid
    }
    fn get_poll<'a>(pid: u32) -> Option<&'a mut PollTarget> {
        unsafe { STATE.poll_table.get_mut(&pid) }
    }
}

impl Preopens for VirtAdapter {
    fn get_directories() -> Vec<(u32, String)> {
        IoState::initialize();
        unsafe { &STATE.preopen_directories }.clone()
    }
}

impl FilesystemTypes for VirtAdapter {
    fn read_via_stream(fd: u32, offset: u64) -> Result<u32, ErrorCode> {
        debug!(
            "CALL wasi:filesystem/types#read_via_stream FD={} OFFSET={}",
            fd, offset
        );
        match IoState::get_descriptor(fd)?.target {
            DescriptorTarget::StaticEntry(_) => {
                if offset != 0 {
                    return Err(ErrorCode::InvalidSeek);
                }
                Ok(IoState::new_stream(StaticFileStream::new(fd)))
            }
            DescriptorTarget::HostDescriptor(host_fd) => {
                let host_sid =
                    filesystem_types::read_via_stream(host_fd, offset).map_err(err_map)?;
                Ok(IoState::new_stream(Stream::Host(host_sid)))
            }
        }
    }
    fn write_via_stream(fd: u32, offset: u64) -> Result<u32, ErrorCode> {
        debug!(
            "CALL wasi:filesystem/types#write_via_stream FD={} OFFSET={}",
            fd, offset
        );
        Err(ErrorCode::Access)
    }
    fn append_via_stream(fd: u32) -> Result<u32, ErrorCode> {
        debug!("CALL wasi:filesystem/types#append_via_stream FD={}", fd);
        Err(ErrorCode::Access)
    }
    fn advise(fd: u32, _: u64, _: u64, _: Advice) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#advise FD={}", fd);
        todo!()
    }
    fn sync_data(fd: u32) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#sync_data FD={}", fd);
        Err(ErrorCode::Access)
    }
    fn get_flags(fd: u32) -> Result<DescriptorFlags, ErrorCode> {
        debug!("CALL wasi:filesystem/types#get_flags FD={}", fd);
        Ok(DescriptorFlags::READ)
    }
    fn get_type(fd: u32) -> Result<DescriptorType, ErrorCode> {
        debug!("CALL wasi:filesystem/types#get_type FD={}", fd);
        IoState::get_descriptor(fd)?.get_type()
    }
    fn set_size(fd: u32, _: u64) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#set_size FD={}", fd);
        Err(ErrorCode::Access)
    }
    fn set_times(fd: u32, _: NewTimestamp, _: NewTimestamp) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#set_times FD={}", fd);
        Err(ErrorCode::Access)
    }
    fn read(fd: u32, len: u64, offset: u64) -> Result<(Vec<u8>, bool), ErrorCode> {
        debug!("CALL wasi:filesystem/types#read FD={}", fd);
        let sid = VirtAdapter::read_via_stream(fd, offset)?;
        let stream = IoState::get_stream(sid).unwrap();
        let Stream::StaticFile(filestream) = stream else {
            unreachable!()
        };
        let (bytes, status) = filestream.read(len).map_err(|_| ErrorCode::Io)?;
        VirtAdapter::drop_input_stream(sid);
        Ok((
            bytes,
            match status {
                StreamStatus::Open => false,
                StreamStatus::Ended => true,
            },
        ))
    }
    fn write(fd: u32, _: Vec<u8>, _: u64) -> Result<u64, ErrorCode> {
        debug!("CALL wasi:filesystem/types#write FD={}", fd);
        Err(ErrorCode::Access)
    }
    fn read_directory(fd: u32) -> Result<u32, ErrorCode> {
        debug!("CALL wasi:filesystem/types#read_directory FD={}", fd);
        let descriptor = IoState::get_descriptor(fd)?;
        if descriptor.get_type()? != DescriptorType::Directory {
            return Err(ErrorCode::NotDirectory);
        }
        Ok(IoState::new_stream(StaticDirStream::new(fd)))
    }
    fn sync(fd: u32) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#sync FD={}", fd);
        Err(ErrorCode::Access)
    }
    fn create_directory_at(fd: u32, path: String) -> Result<(), ErrorCode> {
        debug!(
            "CALL wasi:filesystem/types#create_directory_at FD={} PATH={}",
            fd, &path
        );
        Err(ErrorCode::Access)
    }
    fn stat(fd: u32) -> Result<DescriptorStat, ErrorCode> {
        debug!("CALL wasi:filesystem/types#stat FD={}", &fd);
        let descriptor = IoState::get_descriptor(fd)?;
        match descriptor.target {
            DescriptorTarget::StaticEntry(ptr) => {
                let entry = entry(ptr);
                Ok(DescriptorStat {
                    type_: entry.ty(),
                    link_count: 0,
                    size: entry.size()?,
                    data_access_timestamp: Datetime {
                        seconds: 0,
                        nanoseconds: 0,
                    },
                    data_modification_timestamp: Datetime {
                        seconds: 0,
                        nanoseconds: 0,
                    },
                    status_change_timestamp: Datetime {
                        seconds: 0,
                        nanoseconds: 0,
                    },
                })
            }
            DescriptorTarget::HostDescriptor(host_fd) => filesystem_types::stat(host_fd)
                .map(stat_map)
                .map_err(err_map),
        }
    }
    fn stat_at(fd: u32, flags: PathFlags, path: String) -> Result<DescriptorStat, ErrorCode> {
        debug!(
            "CALL wasi:filesystem/types#stat_at FD={} PATH={}",
            &fd, &path
        );
        let descriptor = IoState::get_descriptor(fd)?;
        match descriptor.target {
            DescriptorTarget::StaticEntry(ptr) => {
                let entry = entry(ptr);
                let child = entry.dir_lookup(&path)?;
                if matches!(
                    child.ty,
                    StaticIndexType::RuntimeDir | StaticIndexType::RuntimeFile
                ) {
                    let Some((host_fd, path)) = IoState::get_host_preopen(child.runtime_path())
                    else {
                        return Err(ErrorCode::NoEntry);
                    };
                    filesystem_types::stat_at(
                        host_fd,
                        filesystem_types::PathFlags::from_bits(flags.bits()).unwrap(),
                        &path,
                    )
                    .map(stat_map)
                    .map_err(err_map)
                } else {
                    Ok(DescriptorStat {
                        type_: child.ty(),
                        link_count: 0,
                        size: child.size()?,
                        data_access_timestamp: Datetime {
                            seconds: 0,
                            nanoseconds: 0,
                        },
                        data_modification_timestamp: Datetime {
                            seconds: 0,
                            nanoseconds: 0,
                        },
                        status_change_timestamp: Datetime {
                            seconds: 0,
                            nanoseconds: 0,
                        },
                    })
                }
            }
            DescriptorTarget::HostDescriptor(host_fd) => filesystem_types::stat_at(
                host_fd,
                filesystem_types::PathFlags::from_bits(flags.bits()).unwrap(),
                &path,
            )
            .map(stat_map)
            .map_err(err_map),
        }
    }
    fn set_times_at(
        fd: u32,
        _: PathFlags,
        path: String,
        _: NewTimestamp,
        _: NewTimestamp,
    ) -> Result<(), ErrorCode> {
        debug!(
            "CALL wasi:filesystem/types#set_times_at FD={} PATH={}",
            fd, &path
        );
        Err(ErrorCode::Access)
    }
    fn link_at(fd: u32, _: PathFlags, path: String, _: u32, _: String) -> Result<(), ErrorCode> {
        debug!(
            "CALL wasi:filesystem/types#link_at FD={} PATH={}",
            fd, &path
        );
        Err(ErrorCode::Access)
    }
    fn open_at(
        fd: u32,
        path_flags: PathFlags,
        path: String,
        open_flags: OpenFlags,
        descriptor_flags: DescriptorFlags,
        modes: Modes,
    ) -> Result<u32, ErrorCode> {
        debug!(
            "CALL wasi:filesystem/types#open_at FD={} PATH={}",
            fd, &path
        );
        let descriptor = IoState::get_descriptor(fd)?;
        match descriptor.target {
            DescriptorTarget::StaticEntry(ptr) => {
                let entry = entry(ptr);
                let child = entry.dir_lookup(&path)?;
                if matches!(
                    child.ty,
                    StaticIndexType::RuntimeDir | StaticIndexType::RuntimeFile
                ) {
                    let Some((host_fd, path)) = IoState::get_host_preopen(child.runtime_path())
                    else {
                        return Err(ErrorCode::NoEntry);
                    };
                    let child_fd = filesystem_types::open_at(
                        host_fd,
                        filesystem_types::PathFlags::from_bits(path_flags.bits()).unwrap(),
                        &path,
                        filesystem_types::OpenFlags::from_bits(open_flags.bits()).unwrap(),
                        filesystem_types::DescriptorFlags::from_bits(descriptor_flags.bits())
                            .unwrap(),
                        filesystem_types::Modes::from_bits(modes.bits()).unwrap(),
                    )
                    .map_err(err_map)?;
                    Ok(IoState::new_descriptor(DescriptorTarget::HostDescriptor(
                        child_fd,
                    )))
                } else {
                    Ok(IoState::new_descriptor(DescriptorTarget::StaticEntry(
                        child,
                    )))
                }
            }
            DescriptorTarget::HostDescriptor(host_fd) => {
                let child_fd = filesystem_types::open_at(
                    host_fd,
                    filesystem_types::PathFlags::from_bits(path_flags.bits()).unwrap(),
                    &path,
                    filesystem_types::OpenFlags::from_bits(open_flags.bits()).unwrap(),
                    filesystem_types::DescriptorFlags::from_bits(descriptor_flags.bits()).unwrap(),
                    filesystem_types::Modes::from_bits(modes.bits()).unwrap(),
                )
                .map_err(err_map)?;
                Ok(IoState::new_descriptor(DescriptorTarget::HostDescriptor(
                    child_fd,
                )))
            }
        }
    }
    fn readlink_at(fd: u32, path: String) -> Result<String, ErrorCode> {
        debug!(
            "CALL wasi:filesystem/types#readlink_ FD={} PATH={}",
            fd, &path
        );
        Ok(path)
    }
    fn remove_directory_at(fd: u32, path: String) -> Result<(), ErrorCode> {
        debug!(
            "CALL wasi:filesystem/types#remove_directory_ FD={} PATH={}",
            fd, &path
        );
        Err(ErrorCode::Access)
    }
    fn rename_at(fd: u32, path: String, _: u32, _: String) -> Result<(), ErrorCode> {
        debug!(
            "CALL wasi:filesystem/types#rename_ FD={} PATH={}",
            fd, &path
        );
        Err(ErrorCode::Access)
    }
    fn symlink_at(fd: u32, path: String, _: String) -> Result<(), ErrorCode> {
        debug!(
            "CALL wasi:filesystem/types#symlink_ FD={} PATH={}",
            fd, &path
        );
        Err(ErrorCode::Access)
    }
    fn access_at(fd: u32, _: PathFlags, path: String, _: AccessType) -> Result<(), ErrorCode> {
        debug!(
            "CALL wasi:filesystem/types#access_ FD={} PATH={}",
            fd, &path
        );
        Err(ErrorCode::Access)
    }
    fn unlink_file_at(fd: u32, path: String) -> Result<(), ErrorCode> {
        debug!(
            "CALL wasi:filesystem/types#unlink_file_ FD={} PATH={}",
            fd, &path
        );
        Err(ErrorCode::Access)
    }
    fn change_file_permissions_at(
        fd: u32,
        _: PathFlags,
        path: String,
        _: Modes,
    ) -> Result<(), ErrorCode> {
        debug!(
            "CALL wasi:filesystem/types#change_file_permissions_ FD={} PATH={}",
            fd, &path
        );
        Err(ErrorCode::Access)
    }
    fn change_directory_permissions_at(
        fd: u32,
        _: PathFlags,
        path: String,
        _: Modes,
    ) -> Result<(), ErrorCode> {
        debug!(
            "CALL wasi:filesystem/types#change_directory_permissions_ FD={} PATH={}",
            fd, &path
        );
        Err(ErrorCode::Access)
    }
    fn lock_shared(fd: u32) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#lock_shared FD={}", fd);
        Ok(())
    }
    fn lock_exclusive(fd: u32) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#lock_exclusive FD={}", fd);
        Ok(())
    }
    fn try_lock_shared(fd: u32) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#try_lock_shared FD={}", fd);
        Ok(())
    }
    fn try_lock_exclusive(fd: u32) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#try_lock_exclusive FD={}", fd);
        Ok(())
    }
    fn unlock(fd: u32) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#unlock FD={}", fd);
        Ok(())
    }
    fn drop_descriptor(fd: u32) {
        debug!("CALL wasi:filesystem/types#drop_descriptor FD={}", fd);
        let Ok(descriptor) = IoState::get_descriptor(fd) else {
            return;
        };
        descriptor.drop();
    }
    fn read_directory_entry(sid: u32) -> Result<Option<DirectoryEntry>, ErrorCode> {
        debug!(
            "CALL wasi:filesystem/types#read_directory_entry SID={}",
            sid
        );
        match IoState::get_stream(sid).map_err(|_| ErrorCode::BadDescriptor)? {
            Stream::StaticDir(dirstream) => dirstream.next(),
            Stream::Host(sid) => filesystem_types::read_directory_entry(*sid)
                .map(|e| e.map(dir_map))
                .map_err(err_map),
            _ => {
                return Err(ErrorCode::BadDescriptor);
            }
        }
    }
    fn drop_directory_entry_stream(sid: u32) {
        debug!(
            "CALL wasi:filesystem/types#drop_directory_entry_stream SID={}",
            sid
        );
        let Ok(stream) = IoState::get_stream(sid) else {
            return;
        };
        match stream {
            Stream::Err | Stream::Null | Stream::StaticFile(_) | Stream::StaticDir(_) => {}
            Stream::Host(sid) => filesystem_types::drop_directory_entry_stream(*sid),
        }
        unsafe { STATE.stream_table.remove(&sid) };
    }

    fn is_same_object(fd1: u32, fd2: u32) -> bool {
        debug!(
            "CALL wasi:filesystem/types#is_same_object FD1={} FD2={}",
            fd1, fd2
        );
        let Ok(descriptor1) = IoState::get_descriptor(fd1) else {
            return false;
        };
        let Ok(descriptor2) = IoState::get_descriptor(fd2) else {
            return false;
        };
        // already-opened static index descriptors will never point to a RuntimeFile
        // or RuntimeDir - instead they point to an already-created HostDescriptor
        match descriptor1.target {
            DescriptorTarget::StaticEntry(entry1) => match descriptor2.target {
                DescriptorTarget::StaticEntry(entry2) => entry1 == entry2,
                DescriptorTarget::HostDescriptor(_) => false,
            },
            DescriptorTarget::HostDescriptor(host_fd1) => match descriptor2.target {
                DescriptorTarget::StaticEntry(_) => false,
                DescriptorTarget::HostDescriptor(host_fd2) => {
                    filesystem_types::is_same_object(host_fd1, host_fd2)
                }
            },
        }
    }

    fn metadata_hash(fd: u32) -> Result<MetadataHashValue, ErrorCode> {
        debug!("CALL wasi:filesystem/types#metadata_hash FD={}", fd);
        let descriptor = IoState::get_descriptor(fd)?;
        match descriptor.target {
            DescriptorTarget::StaticEntry(e) => Ok(MetadataHashValue {
                upper: entry(e).idx() as u64,
                lower: 0,
            }),
            DescriptorTarget::HostDescriptor(host_fd) => filesystem_types::metadata_hash(host_fd)
                .map(metadata_hash_map)
                .map_err(err_map),
        }
    }

    fn metadata_hash_at(
        fd: u32,
        path_flags: PathFlags,
        path: String,
    ) -> Result<MetadataHashValue, ErrorCode> {
        debug!(
            "CALL wasi:filesystem/types#metadata_hash_at FD={} PATH={}",
            fd, &path
        );
        let descriptor = IoState::get_descriptor(fd)?;
        match descriptor.target {
            DescriptorTarget::StaticEntry(ptr) => {
                let entry = entry(ptr);
                let child = entry.dir_lookup(&path)?;
                if matches!(
                    child.ty,
                    StaticIndexType::RuntimeDir | StaticIndexType::RuntimeFile
                ) {
                    let Some((host_fd, path)) = IoState::get_host_preopen(child.runtime_path())
                    else {
                        return Err(ErrorCode::NoEntry);
                    };
                    filesystem_types::metadata_hash_at(
                        host_fd,
                        filesystem_types::PathFlags::from_bits(path_flags.bits()).unwrap(),
                        &path,
                    )
                    .map(metadata_hash_map)
                    .map_err(err_map)
                } else {
                    Ok(MetadataHashValue {
                        upper: child.idx() as u64,
                        lower: 0,
                    })
                }
            }
            DescriptorTarget::HostDescriptor(host_fd) => filesystem_types::metadata_hash_at(
                host_fd,
                filesystem_types::PathFlags::from_bits(path_flags.bits()).unwrap(),
                &path,
            )
            .map(metadata_hash_map)
            .map_err(err_map),
        }
    }
}

fn metadata_hash_map(value: filesystem_types::MetadataHashValue) -> MetadataHashValue {
    MetadataHashValue {
        upper: value.upper,
        lower: value.lower,
    }
}

fn stream_res_map<T>(res: Result<(T, streams::StreamStatus), ()>) -> Result<(T, StreamStatus), ()> {
    match res {
        Ok((data, streams::StreamStatus::Ended)) => Ok((data, StreamStatus::Ended)),
        Ok((data, streams::StreamStatus::Open)) => Ok((data, StreamStatus::Open)),
        Err(_) => Err(()),
    }
}
fn stream_write_res_map<T>(res: Result<T, streams::WriteError>) -> Result<T, WriteError> {
    match res {
        Ok(data) => Ok(data),
        Err(streams::WriteError::Closed) => Err(WriteError::Closed),
        Err(streams::WriteError::LastOperationFailed) => Err(WriteError::LastOperationFailed),
    }
}

impl Streams for VirtAdapter {
    fn read(sid: u32, len: u64) -> Result<(Vec<u8>, StreamStatus), ()> {
        debug!("CALL wasi:io/streams#read SID={}", sid);
        VirtAdapter::blocking_read(sid, len)
    }
    fn blocking_read(sid: u32, len: u64) -> Result<(Vec<u8>, StreamStatus), ()> {
        debug!("CALL wasi:io/streams#blocking_read SID={}", sid);
        let stream = IoState::get_stream(sid)?;
        match stream {
            Stream::StaticFile(filestream) => filestream.read(len),
            Stream::Host(sid) => stream_res_map(streams::blocking_read(*sid, len)),
            Stream::Null => Ok((vec![], StreamStatus::Ended)),
            Stream::Err | Stream::StaticDir(_) => Err(()),
        }
    }
    fn skip(sid: u32, offset: u64) -> Result<(u64, StreamStatus), ()> {
        debug!("CALL wasi:io/streams#skip SID={}", sid);
        match IoState::get_stream(sid)? {
            Stream::Null => Ok((0, StreamStatus::Ended)),
            Stream::Err | Stream::StaticDir(_) | Stream::StaticFile(_) => Err(()),
            Stream::Host(sid) => stream_res_map(streams::skip(*sid, offset)),
        }
    }
    fn blocking_skip(sid: u32, offset: u64) -> Result<(u64, StreamStatus), ()> {
        debug!("CALL wasi:io/streams#blocking_skip SID={}", sid);
        match IoState::get_stream(sid)? {
            Stream::Null => Ok((0, StreamStatus::Ended)),
            Stream::Err | Stream::StaticFile(_) | Stream::StaticDir(_) => Err(()),
            Stream::Host(sid) => stream_res_map(streams::blocking_skip(*sid, offset)),
        }
    }
    fn subscribe_to_input_stream(sid: u32) -> u32 {
        debug!("CALL wasi:io/streams#subscribe_to_input_stream SID={}", sid);
        let Ok(stream) = IoState::get_stream(sid) else {
            panic!()
        };
        match stream {
            Stream::Null => 0,
            Stream::Err | Stream::StaticFile(_) | Stream::StaticDir(_) => 0,
            Stream::Host(sid) => {
                IoState::new_poll(PollTarget::Host(streams::subscribe_to_input_stream(*sid)))
            }
        }
    }
    fn drop_input_stream(sid: u32) {
        debug!("CALL wasi:io/streams#drop_input_stream SID={}", sid);
        let Ok(stream) = IoState::get_stream(sid) else {
            return;
        };
        match stream {
            Stream::Err | Stream::Null | Stream::StaticFile(_) | Stream::StaticDir(_) => {}
            Stream::Host(sid) => streams::drop_input_stream(*sid),
        }
        unsafe { STATE.stream_table.remove(&sid) };
    }
    fn check_write(sid: u32) -> Result<u64, WriteError> {
        debug!("CALL wasi:io/streams#check_write SID={}", sid);
        let Ok(stream) = IoState::get_stream(sid) else {
            return Err(WriteError::Closed);
        };
        match stream {
            Stream::Err | Stream::Null | Stream::StaticFile(_) | Stream::StaticDir(_) => {
                Err(WriteError::Closed)
            }
            Stream::Host(sid) => stream_write_res_map(streams::check_write(*sid)),
        }
    }
    fn write(sid: u32, bytes: Vec<u8>) -> Result<(), WriteError> {
        debug!("CALL wasi:io/streams#write SID={}", sid);
        match IoState::get_stream(sid).map_err(|_| WriteError::Closed)? {
            Stream::Null => Ok(()),
            Stream::Err | Stream::StaticFile(_) | Stream::StaticDir(_) => Err(WriteError::Closed),
            Stream::Host(sid) => stream_write_res_map(streams::write(*sid, bytes.as_slice())),
        }
    }
    fn blocking_write_and_flush(sid: u32, bytes: Vec<u8>) -> Result<(), WriteError> {
        debug!("CALL wasi:io/streams#blocking_write_and_flush SID={}", sid);
        match IoState::get_stream(sid).map_err(|_| WriteError::Closed)? {
            Stream::Null => Ok(()),
            Stream::Err | Stream::StaticFile(_) | Stream::StaticDir(_) => Err(WriteError::Closed),
            Stream::Host(sid) => {
                stream_write_res_map(streams::blocking_write_and_flush(*sid, bytes.as_slice()))
            }
        }
    }
    fn flush(sid: u32) -> Result<(), WriteError> {
        debug!("CALL wasi:io/streams#flush SID={}", sid);
        match IoState::get_stream(sid).map_err(|_| WriteError::Closed)? {
            Stream::Null => Ok(()),
            Stream::Err | Stream::StaticFile(_) | Stream::StaticDir(_) => Err(WriteError::Closed),
            Stream::Host(sid) => stream_write_res_map(streams::flush(*sid)),
        }
    }
    fn blocking_flush(sid: u32) -> Result<(), WriteError> {
        debug!("CALL wasi:io/streams#blocking_flush SID={}", sid);
        match IoState::get_stream(sid).map_err(|_| WriteError::Closed)? {
            Stream::Null => Ok(()),
            Stream::Err | Stream::StaticFile(_) | Stream::StaticDir(_) => Err(WriteError::Closed),
            Stream::Host(sid) => stream_write_res_map(streams::blocking_flush(*sid)),
        }
    }
    fn write_zeroes(sid: u32, len: u64) -> Result<(), WriteError> {
        debug!("CALL wasi:io/streams#write_zeroes SID={}", sid);
        match IoState::get_stream(sid).map_err(|_| WriteError::Closed)? {
            Stream::Null => Ok(()),
            Stream::Err | Stream::StaticFile(_) | Stream::StaticDir(_) => Err(WriteError::Closed),
            Stream::Host(sid) => stream_write_res_map(streams::write_zeroes(*sid, len)),
        }
    }
    fn splice(to_sid: u32, from_sid: u32, len: u64) -> Result<(u64, StreamStatus), ()> {
        debug!(
            "CALL wasi:io/streams#splice TO_SID={} FROM_SID={}",
            to_sid, from_sid
        );
        let to_sid = match IoState::get_stream(to_sid)? {
            Stream::Null => {
                return Ok((len, StreamStatus::Ended));
            }
            Stream::Err | Stream::StaticFile(_) | Stream::StaticDir(_) => {
                return Err(());
            }
            Stream::Host(sid) => *sid,
        };
        let from_sid = match IoState::get_stream(from_sid)? {
            Stream::Null => {
                return Ok((len, StreamStatus::Ended));
            }
            Stream::Err | Stream::StaticFile(_) | Stream::StaticDir(_) => {
                return Err(());
            }
            Stream::Host(sid) => *sid,
        };
        stream_res_map(streams::splice(to_sid, from_sid, len))
    }
    fn blocking_splice(to_sid: u32, from_sid: u32, len: u64) -> Result<(u64, StreamStatus), ()> {
        debug!(
            "CALL wasi:io/streams#blocking_splice TO_SID={} FROM_SID={}",
            to_sid, from_sid
        );
        let to_sid = match IoState::get_stream(to_sid)? {
            Stream::Null => {
                return Ok((len, StreamStatus::Ended));
            }
            Stream::Err | Stream::StaticFile(_) | Stream::StaticDir(_) => {
                return Err(());
            }
            Stream::Host(sid) => *sid,
        };
        let from_sid = match IoState::get_stream(from_sid)? {
            Stream::Null => {
                return Ok((len, StreamStatus::Ended));
            }
            Stream::Err | Stream::StaticFile(_) | Stream::StaticDir(_) => {
                return Err(());
            }
            Stream::Host(sid) => *sid,
        };
        stream_res_map(streams::blocking_splice(to_sid, from_sid, len))
    }
    fn forward(to_sid: u32, from_sid: u32) -> Result<(u64, StreamStatus), ()> {
        debug!(
            "CALL wasi:io/streams#forward TO_SID={} FROM_SID={}",
            to_sid, from_sid
        );
        let to_sid = match IoState::get_stream(to_sid)? {
            Stream::Null => {
                return Ok((0, StreamStatus::Ended));
            }
            Stream::Err | Stream::StaticFile(_) | Stream::StaticDir(_) => {
                return Err(());
            }
            Stream::Host(sid) => *sid,
        };
        let from_sid = match IoState::get_stream(from_sid)? {
            Stream::Null => {
                return Ok((0, StreamStatus::Ended));
            }
            Stream::Err | Stream::StaticFile(_) | Stream::StaticDir(_) => {
                return Err(());
            }
            Stream::Host(sid) => *sid,
        };
        stream_res_map(streams::forward(to_sid, from_sid))
    }
    fn subscribe_to_output_stream(sid: u32) -> u32 {
        debug!(
            "CALL wasi:io/streams#subscribe_to_output_stream SID={}",
            sid
        );
        let Ok(stream) = IoState::get_stream(sid) else {
            panic!();
        };
        match stream {
            Stream::Null => 0,
            Stream::Err | Stream::StaticFile(_) | Stream::StaticDir(_) => 0,
            Stream::Host(sid) => {
                IoState::new_poll(PollTarget::Host(streams::subscribe_to_output_stream(*sid)))
            }
        }
    }
    fn drop_output_stream(sid: u32) {
        debug!("CALL wasi:io/streams#drop_output_stream SID={}", sid);
        let Ok(stream) = IoState::get_stream(sid) else {
            return;
        };
        match stream {
            Stream::Err | Stream::Null | Stream::StaticFile(_) | Stream::StaticDir(_) => {}
            Stream::Host(sid) => streams::drop_output_stream(*sid),
        }
        unsafe { STATE.stream_table.remove(&sid) };
    }
}

// we enforce these descriptor numbers here internally
// then defer to the host descriptor number assignments indirectly
impl Stdin for VirtAdapter {
    fn get_stdin() -> u32 {
        debug!("CALL wasi:cli/stdin#get_stdin");
        0
    }
}

impl Stdout for VirtAdapter {
    fn get_stdout() -> u32 {
        debug!("CALL wasi:cli/stdout#get_stdout");
        1
    }
}

impl Stderr for VirtAdapter {
    fn get_stderr() -> u32 {
        debug!("CALL wasi:cli/stderr#get_stderr");
        2
    }
}

impl TerminalInput for VirtAdapter {
    fn drop_terminal_input(_: u32) {
        debug!("CALL wasi:cli/terminal-input#drop_terminal_input");
    }
}

impl TerminalOutput for VirtAdapter {
    fn drop_terminal_output(_: u32) {
        debug!("CALL wasi:cli/terminal-output#drop_terminal_output");
    }
}

impl TerminalStdin for VirtAdapter {
    fn get_terminal_stdin() -> Option<u32> {
        debug!("CALL wasi:cli/terminal-stdin#get_terminal_stdin");
        Some(0)
    }
}

impl TerminalStdout for VirtAdapter {
    fn get_terminal_stdout() -> Option<u32> {
        debug!("CALL wasi:cli/terminal-stdout#get_terminal_stdout");
        Some(1)
    }
}

impl TerminalStderr for VirtAdapter {
    fn get_terminal_stderr() -> Option<u32> {
        debug!("CALL wasi:cli/terminal-stderr#get_terminal_stderr");
        Some(2)
    }
}

impl Poll for VirtAdapter {
    fn drop_pollable(pid: u32) {
        debug!("CALL wasi:poll/poll#drop_pollable PID={}", pid);
        let Some(poll) = IoState::get_poll(pid) else {
            return;
        };
        match poll {
            PollTarget::Null => {}
            PollTarget::Host(host_pid) => poll::drop_pollable(*host_pid),
        }
        unsafe { STATE.poll_table.remove(&pid) };
    }
    fn poll_oneoff(list: Vec<u32>) -> Vec<bool> {
        debug!(
            "CALL wasi:poll/poll#poll_oneoff PIDS={}",
            list.iter()
                .map(|pid| pid.to_string())
                .collect::<Vec<String>>()
                .join(",")
        );
        let has_host_polls = list
            .iter()
            .find(|&&pid| matches!(IoState::get_poll(pid), Some(PollTarget::Host(_))))
            .is_some();
        let has_virt_polls = list
            .iter()
            .find(|&&pid| matches!(IoState::get_poll(pid), Some(PollTarget::Null)))
            .is_some();
        if has_host_polls && !has_virt_polls {
            return poll::poll_oneoff(&list);
        }
        if has_virt_polls {
            return std::iter::repeat(true).take(list.len()).collect();
        }
        let mut host_polls = Vec::new();
        for pid in &list {
            if let Some(PollTarget::Host(host_pid)) = IoState::get_poll(*pid) {
                host_polls.push(*host_pid);
            }
        }
        let host_ready = poll::poll_oneoff(&host_polls);
        let mut ready = Vec::with_capacity(list.len());
        let mut host_idx = 0;
        for pid in &list {
            match IoState::get_poll(*pid).unwrap() {
                PollTarget::Null => {
                    ready.push(true);
                }
                PollTarget::Host(_) => {
                    ready.push(host_ready[host_idx]);
                    host_idx += 1;
                }
            }
        }
        ready
    }
}

impl MonotonicClock for VirtAdapter {
    fn now() -> u64 {
        debug!("CALL wasi:clocks/monotonic-clock#now");
        monotonic_clock::now()
    }
    fn resolution() -> u64 {
        debug!("CALL wasi:clocks/monotonic-clock#resolution");
        monotonic_clock::resolution()
    }
    fn subscribe(when: u64, absolute: bool) -> u32 {
        debug!("CALL wasi:clocks/monotonic-clock#subscribe");
        let host_pid = monotonic_clock::subscribe(when, absolute);
        IoState::new_poll(PollTarget::Host(host_pid))
    }
}

impl HttpTypes for VirtAdapter {
    fn drop_fields(fields: Fields) {
        debug!("CALL wasi:http/types#drop_fields");
        http_types::drop_fields(fields)
    }
    fn new_fields(entries: Vec<(String, String)>) -> Fields {
        debug!("CALL wasi:http/types#new_fields");
        http_types::new_fields(&entries)
    }
    fn fields_get(fields: Fields, name: String) -> Vec<Vec<u8>> {
        debug!("CALL wasi:http/types#fields_get");
        http_types::fields_get(fields, &name)
    }
    fn fields_set(fields: Fields, name: String, value: Vec<Vec<u8>>) {
        debug!("CALL wasi:http/types#fields_set");
        http_types::fields_set(fields, &name, value.as_slice())
    }
    fn fields_delete(fields: Fields, name: String) {
        debug!("CALL wasi:http/types#fields_delete");
        http_types::fields_delete(fields, &name)
    }
    fn fields_append(fields: Fields, name: String, value: Vec<u8>) {
        debug!("CALL wasi:http/types#fields_append");
        http_types::fields_append(fields, &name, &value)
    }
    fn fields_entries(fields: Fields) -> Vec<(String, Vec<u8>)> {
        debug!("CALL wasi:http/types#fields_entries");
        http_types::fields_entries(fields)
    }
    fn fields_clone(fields: Fields) -> Fields {
        debug!("CALL wasi:http/types#fields_clone");
        http_types::fields_clone(fields)
    }
    fn finish_incoming_stream(s: InputStream) -> Option<Trailers> {
        debug!("CALL wasi:http/types#finish_incoming_stream");
        http_types::finish_incoming_stream(s)
    }
    fn finish_outgoing_stream(s: OutputStream, trailers: Option<Trailers>) {
        debug!("CALL wasi:http/types#finish_outgoing_stream");
        http_types::finish_outgoing_stream(s, trailers)
    }
    fn drop_incoming_request(request: u32) {
        debug!("CALL wasi:http/types#drop_incoming_request");
        http_types::drop_incoming_request(request)
    }
    fn drop_outgoing_request(request: u32) {
        debug!("CALL wasi:http/types#drop_outgoing_request");
        http_types::drop_outgoing_request(request)
    }
    fn incoming_request_method(request: u32) -> Method {
        debug!("CALL wasi:http/types#incoming_request_method");
        method_map_rev(http_types::incoming_request_method(request))
    }
    fn incoming_request_path_with_query(request: u32) -> Option<String> {
        debug!("CALL wasi:http/types#incoming_request_path_with_query");
        http_types::incoming_request_path_with_query(request)
    }
    fn incoming_request_scheme(request: u32) -> Option<Scheme> {
        debug!("CALL wasi:http/types#incoming_request_scheme");
        http_types::incoming_request_scheme(request).map(scheme_map_rev)
    }
    fn incoming_request_authority(request: u32) -> Option<String> {
        debug!("CALL wasi:http/types#incoming_request_authority");
        http_types::incoming_request_authority(request)
    }
    fn incoming_request_headers(request: u32) -> Headers {
        debug!("CALL wasi:http/types#incoming_request_headers");
        http_types::incoming_request_headers(request)
    }
    fn incoming_request_consume(request: u32) -> Result<InputStream, ()> {
        debug!("CALL wasi:http/types#incoming_request_consume");
        http_types::incoming_request_consume(request)
    }
    fn new_outgoing_request(
        method: Method,
        path_with_query: Option<String>,
        scheme: Option<Scheme>,
        authority: Option<String>,
        headers: Headers,
    ) -> u32 {
        debug!("CALL wasi:http/types#new_outgoing_request");
        http_types::new_outgoing_request(
            &method_map(method),
            path_with_query.as_deref(),
            scheme.map(|s| scheme_map(s)).as_ref(),
            authority.as_deref(),
            headers,
        )
    }
    fn outgoing_request_write(request: u32) -> Result<OutputStream, ()> {
        debug!("CALL wasi:http/types#outgoing_request_write");
        http_types::outgoing_request_write(request)
    }
    fn drop_response_outparam(response: u32) {
        debug!("CALL wasi:http/types#drop_response_outparam");
        http_types::drop_response_outparam(response)
    }
    fn set_response_outparam(param: u32, response: Result<u32, Error>) -> Result<(), ()> {
        debug!("CALL wasi:http/types#set_response_outparam");
        match response {
            Ok(res) => http_types::set_response_outparam(param, Ok(res)),
            Err(err) => {
                let err = http_err_map(err);
                http_types::set_response_outparam(param, Err(&err))
            }
        }
    }
    fn drop_incoming_response(response: u32) {
        debug!("CALL wasi:http/types#drop_incoming_response");
        http_types::drop_incoming_response(response)
    }
    fn drop_outgoing_response(response: u32) {
        debug!("CALL wasi:http/types#drop_outgoing_response");
        http_types::drop_outgoing_response(response)
    }
    fn incoming_response_status(response: u32) -> StatusCode {
        debug!("CALL wasi:http/types#incoming_response_status");
        http_types::incoming_response_status(response)
    }
    fn incoming_response_headers(response: u32) -> Headers {
        debug!("CALL wasi:http/types#incoming_response_headers");
        http_types::incoming_response_headers(response)
    }
    fn incoming_response_consume(response: u32) -> Result<InputStream, ()> {
        debug!("CALL wasi:http/types#incoming_response_consume");
        http_types::incoming_response_consume(response)
    }
    fn new_outgoing_response(status_code: StatusCode, headers: Headers) -> u32 {
        debug!("CALL wasi:http/types#new_outgoing_response");
        http_types::new_outgoing_response(status_code, headers)
    }
    fn outgoing_response_write(response: u32) -> Result<OutputStream, ()> {
        debug!("CALL wasi:http/types#outgoing_response_write");
        http_types::outgoing_response_write(response)
    }
    fn drop_future_incoming_response(f: u32) {
        debug!("CALL wasi:http/types#drop_future_incoming_response");
        http_types::drop_future_incoming_response(f)
    }
    fn future_incoming_response_get(f: u32) -> Option<Result<u32, Error>> {
        debug!("CALL wasi:http/types#future_incoming_response_get");
        http_types::future_incoming_response_get(f).map(|o| o.map_err(http_err_map_rev))
    }
    fn listen_to_future_incoming_response(f: u32) -> u32 {
        debug!("CALL wasi:http/types#listen_to_future_incoming_response");
        http_types::listen_to_future_incoming_response(f)
    }
}

fn scheme_map(scheme: Scheme) -> http_types::Scheme {
    match scheme {
        Scheme::Http => http_types::Scheme::Http,
        Scheme::Https => http_types::Scheme::Https,
        Scheme::Other(s) => http_types::Scheme::Other(s),
    }
}

fn scheme_map_rev(scheme: http_types::Scheme) -> Scheme {
    match scheme {
        http_types::Scheme::Http => Scheme::Http,
        http_types::Scheme::Https => Scheme::Https,
        http_types::Scheme::Other(s) => Scheme::Other(s),
    }
}

fn method_map_rev(method: http_types::Method) -> Method {
    match method {
        http_types::Method::Get => Method::Get,
        http_types::Method::Head => Method::Head,
        http_types::Method::Post => Method::Post,
        http_types::Method::Put => Method::Put,
        http_types::Method::Delete => Method::Delete,
        http_types::Method::Connect => Method::Connect,
        http_types::Method::Options => Method::Options,
        http_types::Method::Trace => Method::Trace,
        http_types::Method::Patch => Method::Patch,
        http_types::Method::Other(s) => Method::Other(s),
    }
}

fn method_map(method: Method) -> http_types::Method {
    match method {
        Method::Get => http_types::Method::Get,
        Method::Head => http_types::Method::Head,
        Method::Post => http_types::Method::Post,
        Method::Put => http_types::Method::Put,
        Method::Delete => http_types::Method::Delete,
        Method::Connect => http_types::Method::Connect,
        Method::Options => http_types::Method::Options,
        Method::Trace => http_types::Method::Trace,
        Method::Patch => http_types::Method::Patch,
        Method::Other(s) => http_types::Method::Other(s),
    }
}

fn http_err_map(err: Error) -> http_types::Error {
    match err {
        Error::InvalidUrl(s) => http_types::Error::InvalidUrl(s),
        Error::TimeoutError(s) => http_types::Error::TimeoutError(s),
        Error::ProtocolError(s) => http_types::Error::ProtocolError(s),
        Error::UnexpectedError(s) => http_types::Error::UnexpectedError(s),
    }
}

fn http_err_map_rev(err: http_types::Error) -> Error {
    match err {
        http_types::Error::InvalidUrl(s) => Error::InvalidUrl(s),
        http_types::Error::TimeoutError(s) => Error::TimeoutError(s),
        http_types::Error::ProtocolError(s) => Error::ProtocolError(s),
        http_types::Error::UnexpectedError(s) => Error::UnexpectedError(s),
    }
}

impl IpNameLookup for VirtAdapter {
    fn resolve_addresses(
        network: Network,
        name: String,
        address_family: Option<IpAddressFamily>,
        include_unavailable: bool,
    ) -> Result<ip_name_lookup::ResolveAddressStream, NetworkErrorCode> {
        debug!("CALL wasi:sockets/ip-name-lookup#resolve_addresses");
        ip_name_lookup::resolve_addresses(network, &name, address_family, include_unavailable)
    }
    fn resolve_next_address(
        this: ResolveAddressStream,
    ) -> Result<Option<IpAddress>, NetworkErrorCode> {
        debug!("CALL wasi:sockets/ip-name-lookup#resolve_next_address");
        ip_name_lookup::resolve_next_address(this)
    }
    fn drop_resolve_address_stream(this: ResolveAddressStream) {
        debug!("CALL wasi:sockets/ip-name-lookup#drop_resolve_address_stream");
        ip_name_lookup::drop_resolve_address_stream(this)
    }
    fn subscribe(this: ResolveAddressStream) -> u32 {
        debug!("CALL wasi:sockets/ip-name-lookup#subscribe");
        ip_name_lookup::subscribe(this)
    }
}

impl Tcp for VirtAdapter {
    fn start_bind(
        this: TcpSocket,
        network: Network,
        local_address: IpSocketAddress,
    ) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#start_bind");
        tcp::start_bind(this, network, local_address)
    }
    fn finish_bind(this: TcpSocket) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#finish_bind");
        tcp::finish_bind(this)
    }
    fn start_connect(
        this: TcpSocket,
        network: Network,
        remote_address: IpSocketAddress,
    ) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#start_connect");
        tcp::start_connect(this, network, remote_address)
    }
    fn finish_connect(this: TcpSocket) -> Result<(InputStream, OutputStream), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#finish_connect");
        tcp::finish_connect(this)
    }
    fn start_listen(this: TcpSocket) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#start_listen");
        tcp::start_listen(this)
    }
    fn finish_listen(this: TcpSocket) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#finish_listen");
        tcp::finish_listen(this)
    }
    fn accept(
        this: TcpSocket,
    ) -> Result<(tcp::TcpSocket, InputStream, OutputStream), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#accept");
        tcp::accept(this)
    }
    fn local_address(this: TcpSocket) -> Result<IpSocketAddress, NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#local_address");
        tcp::local_address(this)
    }
    fn remote_address(this: TcpSocket) -> Result<IpSocketAddress, NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#remote_address");
        tcp::remote_address(this)
    }
    fn address_family(this: TcpSocket) -> IpAddressFamily {
        debug!("CALL wasi:sockets/tcp#address_family");
        tcp::address_family(this)
    }
    fn ipv6_only(this: TcpSocket) -> Result<bool, NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#ipv6_only");
        tcp::ipv6_only(this)
    }
    fn set_ipv6_only(this: TcpSocket, value: bool) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#set_ipv6_only");
        tcp::set_ipv6_only(this, value)
    }
    fn set_listen_backlog_size(this: TcpSocket, value: u64) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#set_listen_backlog_size");
        tcp::set_listen_backlog_size(this, value)
    }
    fn keep_alive(this: TcpSocket) -> Result<bool, NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#keep_alive");
        tcp::keep_alive(this)
    }
    fn set_keep_alive(this: TcpSocket, value: bool) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#set_keep_alive");
        tcp::set_keep_alive(this, value)
    }
    fn no_delay(this: TcpSocket) -> Result<bool, NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#no_delay");
        tcp::no_delay(this)
    }
    fn set_no_delay(this: TcpSocket, value: bool) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#set_no_delay");
        tcp::set_no_delay(this, value)
    }
    fn unicast_hop_limit(this: TcpSocket) -> Result<u8, NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#unicast_hop_limit");
        tcp::unicast_hop_limit(this)
    }
    fn set_unicast_hop_limit(this: TcpSocket, value: u8) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#set_unicast_hop_limit");
        tcp::set_unicast_hop_limit(this, value)
    }
    fn receive_buffer_size(this: TcpSocket) -> Result<u64, NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#receive_buffer_size");
        tcp::receive_buffer_size(this)
    }
    fn set_receive_buffer_size(this: TcpSocket, value: u64) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#set_receive_buffer_size");
        tcp::set_receive_buffer_size(this, value)
    }
    fn send_buffer_size(this: TcpSocket) -> Result<u64, NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#send_buffer_size");
        tcp::send_buffer_size(this)
    }
    fn set_send_buffer_size(this: TcpSocket, value: u64) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#set_send_buffer_size");
        tcp::set_send_buffer_size(this, value)
    }
    fn subscribe(this: TcpSocket) -> u32 {
        debug!("CALL wasi:sockets/tcp#subscribe");
        tcp::subscribe(this)
    }
    fn shutdown(this: TcpSocket, shutdown_type: ShutdownType) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#shutdown");
        tcp::shutdown(
            this,
            match shutdown_type {
                ShutdownType::Receive => tcp::ShutdownType::Receive,
                ShutdownType::Send => tcp::ShutdownType::Send,
                ShutdownType::Both => tcp::ShutdownType::Both,
            },
        )
    }
    fn drop_tcp_socket(this: TcpSocket) {
        debug!("CALL wasi:sockets/tcp#drop_tcp_socket");
        tcp::drop_tcp_socket(this)
    }
}

impl Udp for VirtAdapter {
    fn start_bind(
        this: UdpSocket,
        network: Network,
        local_address: IpSocketAddress,
    ) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#start_bind");
        udp::start_bind(this, network, local_address)
    }
    fn finish_bind(this: UdpSocket) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#finish_bind");
        udp::finish_bind(this)
    }
    fn start_connect(
        this: UdpSocket,
        network: Network,
        remote_address: IpSocketAddress,
    ) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#start_connect");
        udp::start_connect(this, network, remote_address)
    }
    fn finish_connect(this: UdpSocket) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#finish_connect");
        udp::finish_connect(this)
    }
    fn receive(this: UdpSocket, max_results: u64) -> Result<Vec<Datagram>, NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#receive");
        match udp::receive(this, max_results) {
            Ok(mut datagrams) => Ok(datagrams
                .drain(..)
                .map(|d| Datagram {
                    data: d.data,
                    remote_address: d.remote_address,
                })
                .collect::<Vec<Datagram>>()),
            Err(err) => Err(err),
        }
    }
    fn send(this: UdpSocket, mut datagrams: Vec<Datagram>) -> Result<u64, NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#send");
        udp::send(
            this,
            datagrams
                .drain(..)
                .map(|d| udp::Datagram {
                    data: d.data,
                    remote_address: d.remote_address,
                })
                .collect::<Vec<udp::Datagram>>()
                .as_slice(),
        )
    }
    fn local_address(this: UdpSocket) -> Result<IpSocketAddress, NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#local_address");
        udp::local_address(this)
    }
    fn remote_address(this: UdpSocket) -> Result<IpSocketAddress, NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#remote_address");
        udp::remote_address(this)
    }
    fn address_family(this: UdpSocket) -> IpAddressFamily {
        debug!("CALL wasi:sockets/udp#address_family");
        udp::address_family(this)
    }
    fn ipv6_only(this: UdpSocket) -> Result<bool, NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#ipv6_only");
        udp::ipv6_only(this)
    }
    fn set_ipv6_only(this: UdpSocket, value: bool) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#set_ipv6_only");
        udp::set_ipv6_only(this, value)
    }
    fn unicast_hop_limit(this: UdpSocket) -> Result<u8, NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#unicast_hop_limit");
        udp::unicast_hop_limit(this)
    }
    fn set_unicast_hop_limit(this: UdpSocket, value: u8) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#set_unicast_hop_limit");
        udp::set_unicast_hop_limit(this, value)
    }
    fn receive_buffer_size(this: UdpSocket) -> Result<u64, NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#receive_buffer_size");
        udp::receive_buffer_size(this)
    }
    fn set_receive_buffer_size(this: UdpSocket, value: u64) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#set_receive_buffer_size");
        udp::set_receive_buffer_size(this, value)
    }
    fn send_buffer_size(this: UdpSocket) -> Result<u64, NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#send_buffer_size");
        udp::send_buffer_size(this)
    }
    fn set_send_buffer_size(this: UdpSocket, value: u64) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#set_send_buffer_size");
        udp::set_send_buffer_size(this, value)
    }
    fn subscribe(this: UdpSocket) -> u32 {
        debug!("CALL wasi:sockets/udp#subscribe");
        udp::subscribe(this)
    }
    fn drop_udp_socket(this: UdpSocket) {
        debug!("CALL wasi:sockets/udp#drop_udp_socket");
        udp::drop_udp_socket(this)
    }
}
