use crate::exports::wasi::cli_base::preopens::Preopens;
use crate::exports::wasi::cli_base::stderr::Stderr;
use crate::exports::wasi::cli_base::stdin::Stdin;
use crate::exports::wasi::cli_base::stdout::Stdout;
use crate::exports::wasi::filesystem::filesystem::{
    AccessType, Advice, Datetime, DescriptorFlags, DescriptorStat, DescriptorType, DirectoryEntry,
    ErrorCode, Filesystem, Modes, NewTimestamp, OpenFlags, PathFlags,
};
use crate::exports::wasi::io::streams::{StreamError, Streams};

use crate::wasi::cli_base::preopens;
use crate::wasi::cli_base::stderr;
use crate::wasi::cli_base::stdin;
use crate::wasi::cli_base::stdout;
use crate::wasi::filesystem::filesystem;
use crate::wasi::io::streams;

use crate::VirtAdapter;

// for debugging
// use crate::console;
// use std::fmt;

use std::alloc::Layout;
use std::cmp;
use std::collections::BTreeMap;
use std::ffi::CStr;
use std::slice;

// io flags
const FLAGS_ENABLE_STDIN: u32 = 1;
const FLAGS_ENABLE_STDOUT: u32 = 2;
const FLAGS_ENABLE_STDERR: u32 = 4;
const FLAGS_HOST_PREOPENS: u32 = 8;
const FLAGS_HOST_PASSTHROUGH: u32 = 16;

// static fs config
#[repr(C)]
pub struct Io {
    preopen_cnt: usize,
    preopens: *const usize,
    static_index_cnt: usize,
    static_index: *const StaticIndexEntry,
    flags: u32,
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
    fn stdin() -> bool {
        (unsafe { io.flags }) & FLAGS_ENABLE_STDIN > 0
    }
    fn stdout() -> bool {
        (unsafe { io.flags }) & FLAGS_ENABLE_STDOUT > 0
    }
    fn stderr() -> bool {
        (unsafe { io.flags }) & FLAGS_ENABLE_STDERR > 0
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

// #[derive(Debug)]
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
            filesystem::drop_descriptor(host_fd);
        }
    }

    fn get_type(&self) -> Result<DescriptorType, ErrorCode> {
        match self.target {
            DescriptorTarget::StaticEntry(ptr) => {
                let entry = entry(ptr);
                Ok(entry.ty())
            }
            DescriptorTarget::HostDescriptor(host_fd) => filesystem::get_type(host_fd)
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
                        // console::log("Internal error: Runtime file should not be reflected directly on descriptors");
                        unreachable!();
                    }
                }
            }
            DescriptorTarget::HostDescriptor(host_fd) => {
                filesystem::read(host_fd, len, offset).map_err(err_map)
            }
        }
    }
}

fn dir_map(d: filesystem::DirectoryEntry) -> DirectoryEntry {
    DirectoryEntry {
        inode: d.inode,
        type_: descriptor_ty_map(d.type_),
        name: d.name,
    }
}

fn stat_map(s: filesystem::DescriptorStat) -> DescriptorStat {
    DescriptorStat {
        device: s.device,
        inode: s.inode,
        type_: descriptor_ty_map(s.type_),
        link_count: s.link_count,
        size: s.size,
        data_access_timestamp: s.data_modification_timestamp,
        data_modification_timestamp: s.data_modification_timestamp,
        status_change_timestamp: s.status_change_timestamp,
    }
}

fn descriptor_ty_map(d: filesystem::DescriptorType) -> DescriptorType {
    match d {
        filesystem::DescriptorType::Unknown => DescriptorType::Unknown,
        filesystem::DescriptorType::BlockDevice => DescriptorType::BlockDevice,
        filesystem::DescriptorType::CharacterDevice => DescriptorType::CharacterDevice,
        filesystem::DescriptorType::Directory => DescriptorType::Directory,
        filesystem::DescriptorType::Fifo => DescriptorType::Fifo,
        filesystem::DescriptorType::SymbolicLink => DescriptorType::SymbolicLink,
        filesystem::DescriptorType::RegularFile => DescriptorType::RegularFile,
        filesystem::DescriptorType::Socket => DescriptorType::Socket,
    }
}

fn err_map(e: filesystem::ErrorCode) -> ErrorCode {
    match e {
        filesystem::ErrorCode::Access => ErrorCode::Access,
        filesystem::ErrorCode::WouldBlock => ErrorCode::WouldBlock,
        filesystem::ErrorCode::Already => ErrorCode::Already,
        filesystem::ErrorCode::BadDescriptor => ErrorCode::BadDescriptor,
        filesystem::ErrorCode::Busy => ErrorCode::Busy,
        filesystem::ErrorCode::Deadlock => ErrorCode::Deadlock,
        filesystem::ErrorCode::Quota => ErrorCode::Quota,
        filesystem::ErrorCode::Exist => ErrorCode::Exist,
        filesystem::ErrorCode::FileTooLarge => ErrorCode::FileTooLarge,
        filesystem::ErrorCode::IllegalByteSequence => ErrorCode::IllegalByteSequence,
        filesystem::ErrorCode::InProgress => ErrorCode::InProgress,
        filesystem::ErrorCode::Interrupted => ErrorCode::Interrupted,
        filesystem::ErrorCode::Invalid => ErrorCode::Invalid,
        filesystem::ErrorCode::Io => ErrorCode::Io,
        filesystem::ErrorCode::IsDirectory => ErrorCode::IsDirectory,
        filesystem::ErrorCode::Loop => ErrorCode::Loop,
        filesystem::ErrorCode::TooManyLinks => ErrorCode::TooManyLinks,
        filesystem::ErrorCode::MessageSize => ErrorCode::MessageSize,
        filesystem::ErrorCode::NameTooLong => ErrorCode::NameTooLong,
        filesystem::ErrorCode::NoDevice => ErrorCode::NoDevice,
        filesystem::ErrorCode::NoEntry => ErrorCode::NoEntry,
        filesystem::ErrorCode::NoLock => ErrorCode::NoLock,
        filesystem::ErrorCode::InsufficientMemory => ErrorCode::InsufficientMemory,
        filesystem::ErrorCode::InsufficientSpace => ErrorCode::InsufficientSpace,
        filesystem::ErrorCode::NotDirectory => ErrorCode::NotDirectory,
        filesystem::ErrorCode::NotEmpty => ErrorCode::NotEmpty,
        filesystem::ErrorCode::NotRecoverable => ErrorCode::NotRecoverable,
        filesystem::ErrorCode::Unsupported => ErrorCode::Unsupported,
        filesystem::ErrorCode::NoTty => ErrorCode::NoTty,
        filesystem::ErrorCode::NoSuchDevice => ErrorCode::NoSuchDevice,
        filesystem::ErrorCode::Overflow => ErrorCode::Overflow,
        filesystem::ErrorCode::NotPermitted => ErrorCode::NotPermitted,
        filesystem::ErrorCode::Pipe => ErrorCode::Pipe,
        filesystem::ErrorCode::ReadOnly => ErrorCode::ReadOnly,
        filesystem::ErrorCode::InvalidSeek => ErrorCode::InvalidSeek,
        filesystem::ErrorCode::TextFileBusy => ErrorCode::TextFileBusy,
        filesystem::ErrorCode::CrossDevice => ErrorCode::CrossDevice,
    }
}

fn entry(ptr: *const StaticIndexEntry) -> &'static StaticIndexEntry {
    unsafe { ptr.as_ref() }.unwrap()
}

impl StaticIndexEntry {
    // fn idx(&self) -> usize {
    //     let static_index_start = unsafe { fs.static_index };
    //     let cur_index_start = self as *const StaticIndexEntry;
    //     unsafe { cur_index_start.sub_ptr(static_index_start) }
    // }
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
                let stat = filesystem::stat_at(fd, filesystem::PathFlags::empty(), subpath)
                    .map_err(err_map)?;
                Ok(stat.size)
            }
        }
    }
    fn child_list(&self) -> Result<&'static [StaticIndexEntry], ErrorCode> {
        if !matches!(self.ty(), DescriptorType::Directory) {
            return Err(ErrorCode::NotDirectory);
        }
        let (child_list_idx, child_list_len) = unsafe { (*self).data.dir };
        let static_index = Io::static_index();
        Ok(&static_index[child_list_idx..child_list_idx + child_list_len])
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

// #[derive(Debug)]
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
    stream_cnt: u32,
    stream_table: BTreeMap<u32, Stream>,
}

static mut STATE: IoState = IoState {
    initialized: false,
    descriptor_cnt: 0,
    preopen_directories: Vec::new(),
    host_preopen_directories: BTreeMap::new(),
    descriptor_table: BTreeMap::new(),
    stream_cnt: 0,
    stream_table: BTreeMap::new(),
};

enum Stream {
    Null,
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
    fn read(&mut self, len: u64) -> Result<(Vec<u8>, bool), StreamError> {
        let descriptor = IoState::get_descriptor(self.fd).map_err(|_| StreamError {})?;
        let (bytes, done) = descriptor
            .read(self.offset, len)
            .map_err(|_| StreamError {})?;
        self.offset += bytes.len() as u64;
        Ok((bytes, done))
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
                inode: None,
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
        IoState::new_stream(if Io::stdin() {
            Stream::Host(stdin::get_stdin())
        } else {
            Stream::Null
        });
        IoState::new_stream(if Io::stdout() {
            Stream::Host(stdout::get_stdout())
        } else {
            Stream::Null
        });
        IoState::new_stream(if Io::stderr() {
            Stream::Host(stderr::get_stderr())
        } else {
            Stream::Null
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
    fn get_stream<'a>(sid: u32) -> Result<&'a mut Stream, StreamError> {
        match unsafe { STATE.stream_table.get_mut(&sid) } {
            Some(stream) => Ok(stream),
            None => Err(StreamError {}),
        }
    }
}

impl Preopens for VirtAdapter {
    fn get_directories() -> Vec<(u32, String)> {
        IoState::initialize();
        unsafe { &STATE.preopen_directories }.clone()
    }
}

impl Filesystem for VirtAdapter {
    fn read_via_stream(fd: u32, offset: u64) -> Result<u32, ErrorCode> {
        match IoState::get_descriptor(fd)?.target {
            DescriptorTarget::StaticEntry(_) => {
                if offset != 0 {
                    return Err(ErrorCode::InvalidSeek);
                }
                Ok(IoState::new_stream(StaticFileStream::new(fd)))
            }
            DescriptorTarget::HostDescriptor(host_fd) => {
                let host_sid = filesystem::read_via_stream(host_fd, offset).map_err(err_map)?;
                Ok(IoState::new_stream(Stream::Host(host_sid)))
            }
        }
    }
    fn write_via_stream(_: u32, _: u64) -> Result<u32, ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn append_via_stream(_fd: u32) -> Result<u32, ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn advise(_: u32, _: u64, _: u64, _: Advice) -> Result<(), ErrorCode> {
        todo!()
    }
    fn sync_data(_: u32) -> Result<(), ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn get_flags(_fd: u32) -> Result<DescriptorFlags, ErrorCode> {
        Ok(DescriptorFlags::READ)
    }
    fn get_type(fd: u32) -> Result<DescriptorType, ErrorCode> {
        IoState::get_descriptor(fd)?.get_type()
    }
    fn set_size(_: u32, _: u64) -> Result<(), ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn set_times(_: u32, _: NewTimestamp, _: NewTimestamp) -> Result<(), ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn read(fd: u32, len: u64, offset: u64) -> Result<(Vec<u8>, bool), ErrorCode> {
        let sid = VirtAdapter::read_via_stream(fd, offset)?;
        let stream = IoState::get_stream(sid).unwrap();
        let Stream::StaticFile(filestream) = stream else {
            unreachable!()
        };
        let result = filestream.read(len).map_err(|_| ErrorCode::Io)?;
        VirtAdapter::drop_input_stream(sid);
        Ok(result)
    }
    fn write(_: u32, _: Vec<u8>, _: u64) -> Result<u64, ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn read_directory(fd: u32) -> Result<u32, ErrorCode> {
        let descriptor = IoState::get_descriptor(fd)?;
        if descriptor.get_type()? != DescriptorType::Directory {
            return Err(ErrorCode::NotDirectory);
        }
        Ok(IoState::new_stream(StaticDirStream::new(fd)))
    }
    fn sync(_: u32) -> Result<(), ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn create_directory_at(_: u32, _: String) -> Result<(), ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn stat(fd: u32) -> Result<DescriptorStat, ErrorCode> {
        let descriptor = IoState::get_descriptor(fd)?;
        match descriptor.target {
            DescriptorTarget::StaticEntry(ptr) => {
                let entry = entry(ptr);
                Ok(DescriptorStat {
                    device: 0,
                    inode: 0,
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
            DescriptorTarget::HostDescriptor(host_fd) => {
                filesystem::stat(host_fd).map(stat_map).map_err(err_map)
            }
        }
    }
    fn stat_at(fd: u32, flags: PathFlags, path: String) -> Result<DescriptorStat, ErrorCode> {
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
                    filesystem::stat_at(
                        host_fd,
                        filesystem::PathFlags::from_bits(flags.bits()).unwrap(),
                        &path,
                    )
                    .map(stat_map)
                    .map_err(err_map)
                } else {
                    Ok(DescriptorStat {
                        device: 0,
                        inode: 0,
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
            DescriptorTarget::HostDescriptor(host_fd) => filesystem::stat_at(
                host_fd,
                filesystem::PathFlags::from_bits(flags.bits()).unwrap(),
                &path,
            )
            .map(stat_map)
            .map_err(err_map),
        }
    }
    fn set_times_at(
        _: u32,
        _: PathFlags,
        _: String,
        _: NewTimestamp,
        _: NewTimestamp,
    ) -> Result<(), ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn link_at(_: u32, _: PathFlags, _: String, _: u32, _: String) -> Result<(), ErrorCode> {
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
                    let child_fd = filesystem::open_at(
                        host_fd,
                        filesystem::PathFlags::from_bits(path_flags.bits()).unwrap(),
                        &path,
                        filesystem::OpenFlags::from_bits(open_flags.bits()).unwrap(),
                        filesystem::DescriptorFlags::from_bits(descriptor_flags.bits()).unwrap(),
                        filesystem::Modes::from_bits(modes.bits()).unwrap(),
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
                let child_fd = filesystem::open_at(
                    host_fd,
                    filesystem::PathFlags::from_bits(path_flags.bits()).unwrap(),
                    &path,
                    filesystem::OpenFlags::from_bits(open_flags.bits()).unwrap(),
                    filesystem::DescriptorFlags::from_bits(descriptor_flags.bits()).unwrap(),
                    filesystem::Modes::from_bits(modes.bits()).unwrap(),
                )
                .map_err(err_map)?;
                Ok(IoState::new_descriptor(DescriptorTarget::HostDescriptor(
                    child_fd,
                )))
            }
        }
    }
    fn readlink_at(_: u32, _: String) -> Result<String, ErrorCode> {
        todo!()
    }
    fn remove_directory_at(_: u32, _: String) -> Result<(), ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn rename_at(_: u32, _: String, _: u32, _: String) -> Result<(), ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn symlink_at(_: u32, _: String, _: String) -> Result<(), ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn access_at(_: u32, _: PathFlags, _: String, _: AccessType) -> Result<(), ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn unlink_file_at(_: u32, _: String) -> Result<(), ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn change_file_permissions_at(
        _: u32,
        _: PathFlags,
        _: String,
        _: Modes,
    ) -> Result<(), ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn change_directory_permissions_at(
        _: u32,
        _: PathFlags,
        _: String,
        _: Modes,
    ) -> Result<(), ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn lock_shared(_fd: u32) -> Result<(), ErrorCode> {
        Ok(())
    }
    fn lock_exclusive(_fd: u32) -> Result<(), ErrorCode> {
        Ok(())
    }
    fn try_lock_shared(_fd: u32) -> Result<(), ErrorCode> {
        Ok(())
    }
    fn try_lock_exclusive(_fd: u32) -> Result<(), ErrorCode> {
        Ok(())
    }
    fn unlock(_: u32) -> Result<(), ErrorCode> {
        Ok(())
    }
    fn drop_descriptor(fd: u32) {
        let Ok(descriptor) = IoState::get_descriptor(fd) else {
            return;
        };
        descriptor.drop();
    }
    fn read_directory_entry(sid: u32) -> Result<Option<DirectoryEntry>, ErrorCode> {
        match IoState::get_stream(sid).map_err(|_| ErrorCode::BadDescriptor)? {
            Stream::StaticDir(dirstream) => dirstream.next(),
            Stream::Host(sid) => filesystem::read_directory_entry(*sid)
                .map(|e| e.map(dir_map))
                .map_err(err_map),
            _ => {
                return Err(ErrorCode::BadDescriptor);
            }
        }
    }
    fn drop_directory_entry_stream(sid: u32) {
        let Ok(stream) = IoState::get_stream(sid) else {
            return;
        };
        match stream {
            Stream::Null | Stream::StaticFile(_) | Stream::StaticDir(_) => {}
            Stream::Host(sid) => filesystem::drop_directory_entry_stream(*sid),
        }
        unsafe { STATE.stream_table.remove(&sid) };
    }
}

impl Streams for VirtAdapter {
    fn read(sid: u32, len: u64) -> Result<(Vec<u8>, bool), StreamError> {
        VirtAdapter::blocking_read(sid, len)
    }
    fn blocking_read(sid: u32, len: u64) -> Result<(Vec<u8>, bool), StreamError> {
        let stream = IoState::get_stream(sid)?;
        match stream {
            Stream::StaticFile(filestream) => filestream.read(len),
            Stream::Host(sid) => streams::blocking_read(*sid, len).map_err(|_| StreamError {}),
            Stream::Null => Ok((vec![], true)),
            Stream::StaticDir(_) => stream_err(),
        }
    }
    fn skip(sid: u32, offset: u64) -> Result<(u64, bool), StreamError> {
        match IoState::get_stream(sid)? {
            Stream::Null => Ok((0, true)),
            Stream::StaticDir(_) | Stream::StaticFile(_) => stream_err(),
            Stream::Host(sid) => streams::skip(*sid, offset).map_err(|_| StreamError {}),
        }
    }
    fn blocking_skip(sid: u32, offset: u64) -> Result<(u64, bool), StreamError> {
        match IoState::get_stream(sid)? {
            Stream::Null => Ok((0, true)),
            Stream::StaticFile(_) | Stream::StaticDir(_) => stream_err(),
            Stream::Host(sid) => streams::blocking_skip(*sid, offset).map_err(|_| StreamError {}),
        }
    }
    fn subscribe_to_input_stream(sid: u32) -> u32 {
        let Ok(stream) = IoState::get_stream(sid) else {
            panic!()
        };
        match stream {
            Stream::Null => 0,
            Stream::StaticFile(_) | Stream::StaticDir(_) => 0,
            Stream::Host(sid) => streams::subscribe_to_input_stream(*sid),
        }
    }
    fn drop_input_stream(sid: u32) {
        let Ok(stream) = IoState::get_stream(sid) else {
            return;
        };
        match stream {
            Stream::Null | Stream::StaticFile(_) | Stream::StaticDir(_) => {}
            Stream::Host(sid) => streams::drop_input_stream(*sid),
        }
        unsafe { STATE.stream_table.remove(&sid) };
    }
    fn write(sid: u32, bytes: Vec<u8>) -> Result<u64, StreamError> {
        match IoState::get_stream(sid)? {
            Stream::Null => Ok(bytes.len() as u64),
            Stream::StaticFile(_) | Stream::StaticDir(_) => stream_err(),
            Stream::Host(sid) => streams::write(*sid, bytes.as_slice()).map_err(|_| StreamError {}),
        }
    }
    fn blocking_write(sid: u32, bytes: Vec<u8>) -> Result<u64, StreamError> {
        match IoState::get_stream(sid)? {
            Stream::Null => Ok(bytes.len() as u64),
            Stream::StaticFile(_) | Stream::StaticDir(_) => stream_err(),
            Stream::Host(sid) => streams::write(*sid, bytes.as_slice()).map_err(|_| StreamError {}),
        }
    }
    fn write_zeroes(sid: u32, len: u64) -> Result<u64, StreamError> {
        match IoState::get_stream(sid)? {
            Stream::Null => Ok(len),
            Stream::StaticFile(_) | Stream::StaticDir(_) => stream_err(),
            Stream::Host(sid) => streams::write_zeroes(*sid, len).map_err(|_| StreamError {}),
        }
    }
    fn blocking_write_zeroes(sid: u32, len: u64) -> Result<u64, StreamError> {
        match IoState::get_stream(sid)? {
            Stream::Null => Ok(len),
            Stream::StaticFile(_) | Stream::StaticDir(_) => stream_err(),
            Stream::Host(sid) => {
                streams::blocking_write_zeroes(*sid, len).map_err(|_| StreamError {})
            }
        }
    }
    fn splice(to_sid: u32, from_sid: u32, len: u64) -> Result<(u64, bool), StreamError> {
        let to_sid = match IoState::get_stream(to_sid)? {
            Stream::Null => {
                return Ok((len, true));
            }
            Stream::StaticFile(_) | Stream::StaticDir(_) => {
                return stream_err();
            }
            Stream::Host(sid) => *sid,
        };
        let from_sid = match IoState::get_stream(from_sid)? {
            Stream::Null => {
                return Ok((len, true));
            }
            Stream::StaticFile(_) | Stream::StaticDir(_) => {
                return stream_err();
            }
            Stream::Host(sid) => *sid,
        };
        streams::splice(to_sid, from_sid, len).map_err(|_| StreamError {})
    }
    fn blocking_splice(to_sid: u32, from_sid: u32, len: u64) -> Result<(u64, bool), StreamError> {
        let to_sid = match IoState::get_stream(to_sid)? {
            Stream::Null => {
                return Ok((len, true));
            }
            Stream::StaticFile(_) | Stream::StaticDir(_) => {
                return stream_err();
            }
            Stream::Host(sid) => *sid,
        };
        let from_sid = match IoState::get_stream(from_sid)? {
            Stream::Null => {
                return Ok((len, true));
            }
            Stream::StaticFile(_) | Stream::StaticDir(_) => {
                return stream_err();
            }
            Stream::Host(sid) => *sid,
        };
        streams::blocking_splice(to_sid, from_sid, len).map_err(|_| StreamError {})
    }
    fn forward(to_sid: u32, from_sid: u32) -> Result<u64, StreamError> {
        let to_sid = match IoState::get_stream(to_sid)? {
            Stream::Null => {
                return Ok(0);
            }
            Stream::StaticFile(_) | Stream::StaticDir(_) => {
                return stream_err();
            }
            Stream::Host(sid) => *sid,
        };
        let from_sid = match IoState::get_stream(from_sid)? {
            Stream::Null => {
                return Ok(0);
            }
            Stream::StaticFile(_) | Stream::StaticDir(_) => {
                return stream_err();
            }
            Stream::Host(sid) => *sid,
        };
        streams::forward(to_sid, from_sid).map_err(|_| StreamError {})
    }
    fn subscribe_to_output_stream(sid: u32) -> u32 {
        let Ok(stream) = IoState::get_stream(sid) else {
            panic!();
        };
        match stream {
            Stream::Null => 0,
            Stream::StaticFile(_) | Stream::StaticDir(_) => 0,
            Stream::Host(sid) => streams::subscribe_to_output_stream(*sid),
        }
    }
    fn drop_output_stream(sid: u32) {
        let Ok(stream) = IoState::get_stream(sid) else {
            return;
        };
        match stream {
            Stream::Null | Stream::StaticFile(_) | Stream::StaticDir(_) => {}
            Stream::Host(sid) => streams::drop_output_stream(*sid),
        }
        unsafe { STATE.stream_table.remove(&sid) };
    }
}

fn stream_err<T>() -> Result<T, StreamError> {
    Err(StreamError {})
}

// we enforce these descriptor numbers here internally
// then defer to the host descriptor number assignments indirectly
impl Stdin for VirtAdapter {
    fn get_stdin() -> u32 {
        0
    }
}

impl Stdout for VirtAdapter {
    fn get_stdout() -> u32 {
        1
    }
}

impl Stderr for VirtAdapter {
    fn get_stderr() -> u32 {
        2
    }
}
