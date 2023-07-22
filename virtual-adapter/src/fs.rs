use crate::exports::wasi::cli_base::preopens::Preopens;
use crate::exports::wasi::filesystem::filesystem::{
    AccessType, Advice, Datetime, DescriptorFlags, DescriptorStat, DescriptorType, DirectoryEntry,
    ErrorCode, Filesystem, Modes, NewTimestamp, OpenFlags, PathFlags,
};
use crate::exports::wasi::io::streams::{StreamError, Streams};
use crate::wasi::cli_base::preopens;
use crate::wasi::filesystem::filesystem;
// use crate::wasi::io::streams;

// for debugging
use crate::console;
// use std::fmt;

use crate::VirtAdapter;
use std::alloc::Layout;
use std::cmp;
use std::collections::BTreeMap;
use std::ffi::CStr;
use std::slice;

// static fs config
#[repr(C)]
pub struct Fs {
    preopen_cnt: usize,
    preopens: *const usize,
    static_index_cnt: usize,
    static_index: *const StaticIndexEntry,
    host_passthrough: bool,
}

impl Fs {
    fn preopens() -> Vec<&'static StaticIndexEntry> {
        let preopen_offsets = unsafe { slice::from_raw_parts(fs.preopens, fs.preopen_cnt) };
        let static_index = Fs::static_index();
        preopen_offsets
            .iter()
            .map(|&idx| &static_index[idx])
            .collect()
    }
    fn static_index() -> &'static [StaticIndexEntry] {
        unsafe { slice::from_raw_parts(fs.static_index, fs.static_index_cnt) }
    }
}

// #[derive(Debug)]
struct Descriptor {
    // the static entry referenced by this descriptor
    entry: *const StaticIndexEntry,
    // the descriptor index of this descriptor
    fd: u32,
    // if a host entry, the underlying host descriptor
    // (if any)
    host_fd: Option<u32>,
}

impl Descriptor {
    fn entry(&self) -> &StaticIndexEntry {
        unsafe { self.entry.as_ref() }.unwrap()
    }

    fn drop(&self) {
        unsafe {
            STATE.descriptor_table.remove(&self.fd);
        }
        if let Some(host_fd) = self.host_fd {
            filesystem::drop_descriptor(host_fd);
        }
    }

    fn get_bytes<'a>(&mut self, offset: u64, len: u64) -> Result<(Vec<u8>, bool), ErrorCode> {
        let entry = self.entry();
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
                unsafe { std::alloc::dealloc(data, Layout::from_size_align(1, 4).unwrap()) };
                Ok((vec, read_len < len as usize))
            }
            StaticIndexType::Dir => todo!(),
            StaticIndexType::RuntimeDir => todo!(),
            StaticIndexType::RuntimeFile => {
                if let Some(host_fd) = self.host_fd {
                    return filesystem::read(host_fd, len, offset).map_err(err_map);
                }

                let path = unsafe { CStr::from_ptr(entry.data.runtime_path) };
                let path = path.to_str().unwrap();

                let Some((preopen_fd, subpath)) = FsState::get_host_preopen(path) else {
                    return Err(ErrorCode::NoEntry);
                };
                let host_fd = filesystem::open_at(
                    preopen_fd,
                    filesystem::PathFlags::empty(),
                    subpath,
                    filesystem::OpenFlags::empty(),
                    filesystem::DescriptorFlags::READ,
                    filesystem::Modes::READABLE,
                )
                .map_err(err_map)?;

                self.host_fd = Some(host_fd);
                filesystem::read(host_fd, len, offset).map_err(err_map)
            }
        }
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

impl StaticIndexEntry {
    // fn idx(&self) -> usize {
    //     let static_index_start = unsafe { fs.static_index };
    //     let cur_index_start = self as *const StaticIndexEntry;
    //     unsafe { cur_index_start.sub_ptr(static_index_start) }
    // }
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
                let path = unsafe { CStr::from_ptr(self.data.runtime_path) };
                let path = path.to_str().unwrap();
                let Some((fd, subpath)) = FsState::get_host_preopen(path) else {
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
        let static_index = Fs::static_index();
        Ok(&static_index[child_list_idx..child_list_idx + child_list_len])
    }
    fn dir_lookup(&self, path: &str) -> Result<&StaticIndexEntry, ErrorCode> {
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
pub static mut fs: Fs = Fs {
    preopen_cnt: 0,                             // [byte 0]
    preopens: 0 as *const usize,                // [byte 4]
    static_index_cnt: 0,                        // [byte 8]
    static_index: 0 as *const StaticIndexEntry, // [byte 12]
    host_passthrough: false,                    // [byte 16]
};

// local fs state
pub struct FsState {
    initialized: bool,
    descriptor_cnt: u32,
    preopen_directories: Vec<u32>,
    host_preopen_directories: BTreeMap<String, u32>,
    descriptor_table: BTreeMap<u32, Descriptor>,
    stream_cnt: u32,
    stream_table: BTreeMap<u32, Stream>,
}

static mut STATE: FsState = FsState {
    initialized: false,
    descriptor_cnt: 3,
    preopen_directories: Vec::new(),
    host_preopen_directories: BTreeMap::new(),
    descriptor_table: BTreeMap::new(),
    stream_cnt: 0,
    stream_table: BTreeMap::new(),
};

enum Stream {
    File(FileStream),
    Dir(DirStream),
}

impl From<FileStream> for Stream {
    fn from(value: FileStream) -> Self {
        Stream::File(value)
    }
}

impl From<DirStream> for Stream {
    fn from(value: DirStream) -> Self {
        Stream::Dir(value)
    }
}

struct FileStream {
    // local file descriptor
    fd: u32,
    // current offset
    offset: u64,
}

struct DirStream {
    fd: u32,
    idx: usize,
}

impl FileStream {
    fn new(fd: u32) -> Self {
        Self { fd, offset: 0 }
    }
    fn read(&mut self, len: u64) -> Result<(Vec<u8>, bool), StreamError> {
        let Some(descriptor) = FsState::get_descriptor(self.fd) else {
            return Err(StreamError {});
        };
        let (bytes, done) = descriptor
            .get_bytes(self.offset, len)
            .map_err(|_| StreamError {})?;
        self.offset += bytes.len() as u64;
        Ok((bytes, done))
    }
}

impl DirStream {
    fn new(fd: u32) -> Self {
        Self { fd, idx: 0 }
    }
    fn next(&mut self) -> Result<Option<DirectoryEntry>, ErrorCode> {
        let Some(descriptor) = FsState::get_descriptor(self.fd) else {
            return Err(ErrorCode::BadDescriptor);
        };
        let child_list = descriptor.entry().child_list()?;
        if self.idx < child_list.len() {
            let child = &child_list[self.idx];
            self.idx += 1;
            Ok(Some(DirectoryEntry {
                inode: None,
                type_: child.ty(),
                name: child.name().into(),
            }))
        } else {
            Ok(None)
        }
    }
}

impl FsState {
    fn initialize() {
        if unsafe { STATE.initialized } {
            return;
        }
        if unsafe { fs.host_passthrough } {
            let host_preopen_directories = unsafe { &mut STATE.host_preopen_directories };
            for (fd, name) in preopens::get_directories() {
                host_preopen_directories.insert(name, fd);
            }
        }
        let preopens = Fs::preopens();
        for preopen in preopens {
            let fd = FsState::create_descriptor(preopen, DescriptorFlags::READ);
            unsafe { STATE.preopen_directories.push(fd) }
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
    fn create_descriptor(entry: &StaticIndexEntry, _flags: DescriptorFlags) -> u32 {
        let fd = unsafe { STATE.descriptor_cnt };
        unsafe { STATE.descriptor_cnt += 1 };
        let descriptor = Descriptor {
            entry,
            fd,
            host_fd: None,
        };
        assert!(unsafe { STATE.descriptor_table.insert(fd, descriptor) }.is_none());
        fd
    }
    fn get_descriptor<'a>(fd: u32) -> Option<&'a mut Descriptor> {
        unsafe { STATE.descriptor_table.get_mut(&fd) }
    }
    fn get_preopen_directories() -> Vec<(u32, String)> {
        FsState::initialize();
        unsafe { &STATE.preopen_directories }
            .iter()
            .map(|&fd| {
                let descriptor = FsState::get_descriptor(fd).unwrap();
                let name = descriptor.entry().name();
                (fd, name.to_string())
            })
            .collect()
    }
    fn create_stream<S: Into<Stream>>(stream: S) -> Result<u32, ErrorCode> {
        let sid = unsafe { STATE.stream_cnt };
        unsafe { STATE.stream_cnt += 1 };
        unsafe { STATE.stream_table.insert(sid, stream.into()) };
        Ok(sid)
    }
    fn get_stream<'a>(sid: u32) -> Option<&'a mut Stream> {
        unsafe { STATE.stream_table.get_mut(&sid) }
    }
    fn drop_stream(sid: u32) {
        unsafe { STATE.stream_table.remove(&sid) };
    }
}

impl Preopens for VirtAdapter {
    fn get_directories() -> Vec<(u32, String)> {
        FsState::get_preopen_directories()
    }
}

impl Filesystem for VirtAdapter {
    fn read_via_stream(fd: u32, offset: u64) -> Result<u32, ErrorCode> {
        if offset != 0 {
            return Err(ErrorCode::InvalidSeek);
        }
        FsState::create_stream(FileStream::new(fd))
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
        let Some(descriptor) = FsState::get_descriptor(fd) else {
            return Err(ErrorCode::BadDescriptor);
        };
        Ok(descriptor.entry().ty())
    }
    fn set_size(_: u32, _: u64) -> Result<(), ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn set_times(_: u32, _: NewTimestamp, _: NewTimestamp) -> Result<(), ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn read(_: u32, _: u64, _: u64) -> Result<(Vec<u8>, bool), ErrorCode> {
        todo!()
    }
    fn write(_: u32, _: Vec<u8>, _: u64) -> Result<u64, ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn read_directory(fd: u32) -> Result<u32, ErrorCode> {
        let Some(descriptor) = FsState::get_descriptor(fd) else {
            return Err(ErrorCode::BadDescriptor);
        };
        if descriptor.entry().ty() != DescriptorType::Directory {
            return Err(ErrorCode::NotDirectory);
        }
        FsState::create_stream(DirStream::new(fd))
    }
    fn sync(_: u32) -> Result<(), ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn create_directory_at(_: u32, _: String) -> Result<(), ErrorCode> {
        Err(ErrorCode::Access)
    }
    fn stat(fd: u32) -> Result<DescriptorStat, ErrorCode> {
        let Some(descriptor) = FsState::get_descriptor(fd) else {
            return Err(ErrorCode::BadDescriptor);
        };
        Ok(DescriptorStat {
            device: 0,
            inode: 0,
            type_: descriptor.entry().ty(),
            link_count: 0,
            size: descriptor.entry().size()?,
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
    fn stat_at(fd: u32, _flags: PathFlags, path: String) -> Result<DescriptorStat, ErrorCode> {
        let Some(descriptor) = FsState::get_descriptor(fd) else {
            return Err(ErrorCode::BadDescriptor);
        };
        let child = descriptor.entry().dir_lookup(&path)?;
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
        _path_flags: PathFlags,
        path: String,
        _open_flags: OpenFlags,
        descriptor_flags: DescriptorFlags,
        _modes: Modes,
    ) -> Result<u32, ErrorCode> {
        let Some(descriptor) = FsState::get_descriptor(fd) else {
            return Err(ErrorCode::BadDescriptor);
        };
        let child = descriptor.entry().dir_lookup(&path)?;
        Ok(FsState::create_descriptor(child, descriptor_flags))
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
        if let Some(descriptor) = FsState::get_descriptor(fd) {
            descriptor.drop();
        };
    }
    fn read_directory_entry(sid: u32) -> Result<Option<DirectoryEntry>, ErrorCode> {
        let Some(stream) = FsState::get_stream(sid) else {
            return Err(ErrorCode::BadDescriptor);
        };
        match stream {
            Stream::Dir(dirstream) => dirstream.next(),
            _ => {
                return Err(ErrorCode::BadDescriptor);
            }
        }
    }
    fn drop_directory_entry_stream(sid: u32) {
        FsState::drop_stream(sid);
    }
}

impl Streams for VirtAdapter {
    fn read(_: u32, _: u64) -> Result<(Vec<u8>, bool), StreamError> {
        todo!()
    }
    fn blocking_read(sid: u32, len: u64) -> Result<(Vec<u8>, bool), StreamError> {
        let Some(stream) = FsState::get_stream(sid) else {
            return Err(StreamError {});
        };
        match stream {
            Stream::File(filestream) => filestream.read(len),
            _ => Err(StreamError {}),
        }
    }
    fn skip(_: u32, _: u64) -> Result<(u64, bool), StreamError> {
        todo!()
    }
    fn blocking_skip(_: u32, _: u64) -> Result<(u64, bool), StreamError> {
        todo!()
    }
    fn subscribe_to_input_stream(_: u32) -> u32 {
        todo!()
    }
    fn drop_input_stream(sid: u32) {
        FsState::drop_stream(sid);
    }
    fn write(_: u32, _: Vec<u8>) -> Result<u64, StreamError> {
        Err(StreamError {})
    }
    fn blocking_write(_: u32, _: Vec<u8>) -> Result<u64, StreamError> {
        Err(StreamError {})
    }
    fn write_zeroes(_: u32, _: u64) -> Result<u64, StreamError> {
        Err(StreamError {})
    }
    fn blocking_write_zeroes(_: u32, _: u64) -> Result<u64, StreamError> {
        Err(StreamError {})
    }
    fn splice(_: u32, _: u32, _: u64) -> Result<(u64, bool), StreamError> {
        todo!()
    }
    fn blocking_splice(_: u32, _: u32, _: u64) -> Result<(u64, bool), StreamError> {
        todo!()
    }
    fn forward(_: u32, _: u32) -> Result<u64, StreamError> {
        todo!()
    }
    fn subscribe_to_output_stream(_: u32) -> u32 {
        todo!()
    }
    fn drop_output_stream(_: u32) {
        todo!()
    }
}
