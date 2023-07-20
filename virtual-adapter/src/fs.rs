use crate::exports::wasi::cli_base::preopens::Preopens;
use crate::exports::wasi::filesystem::filesystem::{
    AccessType, Advice, Datetime, DescriptorFlags, DescriptorStat, DescriptorType, DirectoryEntry,
    ErrorCode, Filesystem, Modes, NewTimestamp, OpenFlags, PathFlags,
};
use crate::exports::wasi::io::streams::{StreamError, Streams};
// use crate::wasi::cli_base::preopens;
// use crate::wasi::filesystem::filesystem;
// use crate::wasi::io::streams;

// for debugging
// use crate::console;
// use std::fmt;

use crate::VirtAdapter;
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
    entry: *const StaticIndexEntry,
}

impl Descriptor {
    fn entry(&self) -> &StaticIndexEntry {
        unsafe { self.entry.as_ref() }.unwrap()
    }
}

impl StaticIndexEntry {
    #[allow(dead_code)]
    fn idx(&self) -> usize {
        let static_index_start = unsafe { fs.static_index };
        let cur_index_start = self as *const StaticIndexEntry;
        unsafe { cur_index_start.sub_ptr(static_index_start) }
    }
    fn name(&self) -> &'static str {
        let c_str = unsafe { CStr::from_ptr((*self).name) };
        c_str.to_str().unwrap()
    }
    fn ty(&self) -> DescriptorType {
        match self.ty {
            StaticIndexType::RuntimeHostFile => todo!(),
            StaticIndexType::ActiveFile => DescriptorType::RegularFile,
            StaticIndexType::PassiveFile => DescriptorType::RegularFile,
            StaticIndexType::RuntimeHostDir => todo!(),
            StaticIndexType::Dir => DescriptorType::Directory,
        }
    }
    fn size(&self) -> usize {
        match self.ty {
            StaticIndexType::ActiveFile => unsafe { self.data.active.1 },
            StaticIndexType::PassiveFile => unsafe { self.data.passive.1 },
            StaticIndexType::Dir => 0,
            StaticIndexType::RuntimeHostDir => 0,
            StaticIndexType::RuntimeHostFile => todo!(),
        }
    }
    fn get_bytes<'a>(&'a self) -> &'a [u8] {
        match self.ty {
            StaticIndexType::ActiveFile => unsafe {
                slice::from_raw_parts(self.data.active.0, self.data.active.1)
            },
            StaticIndexType::PassiveFile => {
                let passive_idx = unsafe { self.data.passive.0 };
                let passive_len = unsafe { self.data.passive.1 };
                let data = passive_alloc(passive_idx, 0, passive_len as u32);
                unsafe { slice::from_raw_parts(data, passive_len) }
            }
            StaticIndexType::Dir => todo!(),
            StaticIndexType::RuntimeHostDir => todo!(),
            StaticIndexType::RuntimeHostFile => todo!(),
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
    path: *const u8,
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
    RuntimeHostDir,
    RuntimeHostFile,
}

// This function gets mutated by the virtualizer
#[no_mangle]
#[inline(never)]
pub fn passive_alloc(passive_idx: u32, offset: u32, len: u32) -> *const u8 {
    return (passive_idx + offset + len) as *const u8;
}

#[no_mangle]
pub static mut fs: Fs = Fs {
    preopen_cnt: 0,                             // [byte 0]
    preopens: 0 as *const usize,                // [byte 4]
    static_index_cnt: 0,                        // [byte 8]
    static_index: 0 as *const StaticIndexEntry, // [byte 12]
};

// local fs state
pub struct FsState {
    initialized: bool,
    descriptor_cnt: u32,
    preopen_directories: Vec<u32>,
    descriptor_table: BTreeMap<u32, Descriptor>,
    stream_cnt: u32,
    stream_table: BTreeMap<u32, Stream>,
}

static mut STATE: FsState = FsState {
    initialized: false,
    descriptor_cnt: 3,
    preopen_directories: Vec::new(),
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
    fd: u32,
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
    fn read(&mut self, len: u64) -> Result<Option<Vec<u8>>, StreamError> {
        let Some(descriptor) = FsState::get_descriptor(self.fd) else {
            return Err(StreamError {});
        };
        let bytes = descriptor.entry().get_bytes();
        let read_len = cmp::min(bytes.len() as u64 - self.offset, len);
        if read_len == 0 {
            return Ok(None);
        }
        let byte_slice = &bytes[self.offset as usize..(self.offset + read_len) as usize];
        self.offset += read_len;
        Ok(Some(byte_slice.to_vec()))
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
        // TODO: Host passthrough
        // let _host_preopen_directories = Some(preopens::get_directories());
        let preopens = Fs::preopens();
        for preopen in preopens {
            let fd = FsState::create_descriptor(preopen, DescriptorFlags::READ);
            unsafe { STATE.preopen_directories.push(fd) }
        }
        unsafe { STATE.initialized = true };
    }
    fn create_descriptor(entry: &StaticIndexEntry, _flags: DescriptorFlags) -> u32 {
        let fd = unsafe { STATE.descriptor_cnt };
        unsafe { STATE.descriptor_cnt += 1 };
        let descriptor = Descriptor { entry };
        assert!(unsafe { STATE.descriptor_table.insert(fd, descriptor) }.is_none());
        fd
    }
    fn get_descriptor<'a>(fd: u32) -> Option<&'a Descriptor> {
        unsafe { STATE.descriptor_table.get(&fd) }
    }
    fn drop_descriptor(fd: u32) {
        unsafe {
            STATE.descriptor_table.remove(&fd);
        }
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
        todo!()
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
        todo!()
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
            size: descriptor.entry().size() as u64,
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
            size: child.size() as u64,
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
        todo!()
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
        todo!()
    }
    fn rename_at(_: u32, _: String, _: u32, _: String) -> Result<(), ErrorCode> {
        todo!()
    }
    fn symlink_at(_: u32, _: String, _: String) -> Result<(), ErrorCode> {
        todo!()
    }
    fn access_at(_: u32, _: PathFlags, _: String, _: AccessType) -> Result<(), ErrorCode> {
        todo!()
    }
    fn unlink_file_at(_: u32, _: String) -> Result<(), ErrorCode> {
        todo!()
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
        FsState::drop_descriptor(fd);
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
            Stream::File(filestream) => match filestream.read(len)? {
                Some(vec) => Ok((vec, false)),
                None => Ok((vec![], true)),
            },
            _ => {
                return Err(StreamError {});
            }
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
