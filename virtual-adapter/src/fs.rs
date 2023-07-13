use crate::exports::wasi::cli_base::preopens::Preopens;
use crate::exports::wasi::filesystem::filesystem::{
    AccessType, Advice, DescriptorFlags, DescriptorStat, DescriptorType, DirectoryEntry, ErrorCode,
    Filesystem, Modes, NewTimestamp, OpenFlags, PathFlags,
};
use crate::exports::wasi::io::streams::{StreamError, Streams};
use crate::wasi::cli_base::preopens;
use crate::wasi::filesystem::filesystem;
use crate::wasi::io::streams;
use crate::VirtAdapter;
use std::ffi::CStr;

// static fs config
#[repr(C)]
pub struct Fs {
    preopen_cnt: usize,
    preopens: *const (u32, *const i8),
}

#[no_mangle]
pub static mut fs: Fs = Fs {
    preopen_cnt: 0,
    preopens: 0 as *const (u32, *const i8),
};

// local fs state
pub struct FsState {
    host_preopen_directories: Option<Vec<(u32, String)>>,
}

static mut STATE: FsState = FsState {
    host_preopen_directories: None,
};

impl Preopens for VirtAdapter {
    fn get_directories() -> Vec<(u32, String)> {
        unsafe { STATE.host_preopen_directories = Some(preopens::get_directories()) };
        let mut preopens: Vec<(u32, String)> =
            unsafe { std::slice::from_raw_parts(fs.preopens, fs.preopen_cnt) }
                .iter()
                .map(|(fd, path)| {
                    (
                        *fd,
                        unsafe { CStr::from_ptr(*path) }
                            .to_str()
                            .unwrap()
                            .to_string(),
                    )
                })
                .collect();
        preopens.push((3, "/".into()));
        preopens
    }
}

impl Filesystem for VirtAdapter {
    fn read_via_stream(_: u32, _: u64) -> Result<u32, ErrorCode> {
        todo!()
    }
    fn write_via_stream(_: u32, _: u64) -> Result<u32, ErrorCode> {
        todo!()
    }
    fn append_via_stream(_: u32) -> Result<u32, ErrorCode> {
        todo!()
    }
    fn advise(_: u32, _: u64, _: u64, _: Advice) -> Result<(), ErrorCode> {
        todo!()
    }
    fn sync_data(_: u32) -> Result<(), ErrorCode> {
        todo!()
    }
    fn get_flags(_: u32) -> Result<DescriptorFlags, ErrorCode> {
        todo!()
    }
    fn get_type(_: u32) -> Result<DescriptorType, ErrorCode> {
        todo!()
    }
    fn set_size(_: u32, _: u64) -> Result<(), ErrorCode> {
        todo!()
    }
    fn set_times(_: u32, _: NewTimestamp, _: NewTimestamp) -> Result<(), ErrorCode> {
        todo!()
    }
    fn read(_: u32, _: u64, _: u64) -> Result<(Vec<u8>, bool), ErrorCode> {
        todo!()
    }
    fn write(_: u32, _: Vec<u8>, _: u64) -> Result<u64, ErrorCode> {
        todo!()
    }
    fn read_directory(_: u32) -> Result<u32, ErrorCode> {
        todo!()
    }
    fn sync(_: u32) -> Result<(), ErrorCode> {
        todo!()
    }
    fn create_directory_at(_: u32, _: String) -> Result<(), ErrorCode> {
        todo!()
    }
    fn stat(_: u32) -> Result<DescriptorStat, ErrorCode> {
        todo!()
    }
    fn stat_at(_: u32, _: PathFlags, _: String) -> Result<DescriptorStat, ErrorCode> {
        todo!()
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
        todo!()
    }
    fn open_at(
        _: u32,
        _: PathFlags,
        _: String,
        _: OpenFlags,
        _: DescriptorFlags,
        _: Modes,
    ) -> Result<u32, ErrorCode> {
        todo!()
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
        todo!()
    }
    fn change_directory_permissions_at(
        _: u32,
        _: PathFlags,
        _: String,
        _: Modes,
    ) -> Result<(), ErrorCode> {
        todo!()
    }
    fn lock_shared(_: u32) -> Result<(), ErrorCode> {
        todo!()
    }
    fn lock_exclusive(_: u32) -> Result<(), ErrorCode> {
        todo!()
    }
    fn try_lock_shared(_: u32) -> Result<(), ErrorCode> {
        todo!()
    }
    fn try_lock_exclusive(_: u32) -> Result<(), ErrorCode> {
        todo!()
    }
    fn unlock(_: u32) -> Result<(), ErrorCode> {
        todo!()
    }
    fn drop_descriptor(_: u32) {
        todo!()
    }
    fn read_directory_entry(_: u32) -> Result<Option<DirectoryEntry>, ErrorCode> {
        todo!()
    }
    fn drop_directory_entry_stream(_: u32) {
        todo!()
    }
}

impl Streams for VirtAdapter {
    fn read(_: u32, _: u64) -> Result<(Vec<u8>, bool), StreamError> {
        todo!()
    }
    fn blocking_read(_: u32, _: u64) -> Result<(Vec<u8>, bool), StreamError> {
        todo!()
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
    fn drop_input_stream(_: u32) {
        todo!()
    }
    fn write(_: u32, _: Vec<u8>) -> Result<u64, StreamError> {
        todo!()
    }
    fn blocking_write(_: u32, _: Vec<u8>) -> Result<u64, StreamError> {
        todo!()
    }
    fn write_zeroes(_: u32, _: u64) -> Result<u64, StreamError> {
        todo!()
    }
    fn blocking_write_zeroes(_: u32, _: u64) -> Result<u64, StreamError> {
        todo!()
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
