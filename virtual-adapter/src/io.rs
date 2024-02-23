use crate::exports::wasi::cli::stderr::Guest as Stderr;
use crate::exports::wasi::cli::stdin::Guest as Stdin;
use crate::exports::wasi::cli::stdout::Guest as Stdout;
use crate::exports::wasi::cli::terminal_input::TerminalInput;
use crate::exports::wasi::cli::terminal_output::TerminalOutput;
use crate::exports::wasi::cli::terminal_stderr::Guest as TerminalStderr;
use crate::exports::wasi::cli::terminal_stdin::Guest as TerminalStdin;
use crate::exports::wasi::cli::terminal_stdout::Guest as TerminalStdout;
use crate::exports::wasi::clocks::monotonic_clock::Guest as MonotonicClock;
use crate::exports::wasi::filesystem::preopens::Guest as Preopens;
use crate::exports::wasi::filesystem::types::{
    Advice, Descriptor, DescriptorFlags, DescriptorStat, DescriptorType, DirectoryEntry,
    DirectoryEntryStream, ErrorCode, Guest as FilesystemTypes, GuestDescriptor,
    GuestDirectoryEntryStream, MetadataHashValue, NewTimestamp, OpenFlags, PathFlags,
};
use crate::exports::wasi::http::outgoing_handler::Guest as OutgoingHandler;
use crate::exports::wasi::http::types::{
    DnsErrorPayload, ErrorCode as HttpErrorCode, FieldSizePayload, Fields, FutureIncomingResponse,
    FutureTrailers, Guest as GuestHttpTypes, GuestFields, GuestFutureIncomingResponse,
    GuestFutureTrailers, GuestIncomingBody, GuestIncomingRequest, GuestIncomingResponse,
    GuestOutgoingBody, GuestOutgoingRequest, GuestOutgoingResponse, GuestRequestOptions,
    GuestResponseOutparam, HeaderError, Headers, IncomingBody, IncomingRequest, IncomingResponse,
    Method, OutgoingBody, OutgoingRequest, OutgoingResponse, RequestOptions, ResponseOutparam,
    Scheme, StatusCode, TlsAlertReceivedPayload,
};
use crate::exports::wasi::io::error::GuestError as GuestStreamsError;
use crate::exports::wasi::io::poll::{Guest as Poll, GuestPollable, Pollable};
use crate::exports::wasi::io::streams::{
    Error as StreamsError, GuestInputStream, GuestOutputStream, InputStream, OutputStream,
    StreamError,
};
use crate::exports::wasi::sockets::ip_name_lookup::{
    Guest as IpNameLookup, GuestResolveAddressStream, IpAddress, Network, ResolveAddressStream,
};
use crate::exports::wasi::sockets::tcp::{
    Duration, ErrorCode as NetworkErrorCode, GuestTcpSocket, IpAddressFamily, IpSocketAddress,
    ShutdownType, TcpSocket,
};
use crate::exports::wasi::sockets::udp::{
    GuestIncomingDatagramStream, GuestOutgoingDatagramStream, GuestUdpSocket, IncomingDatagram,
    OutgoingDatagram, UdpSocket,
};

use crate::wasi::cli::stdin;
use crate::wasi::cli::stdout;
use crate::wasi::cli::terminal_input;
use crate::wasi::cli::terminal_output;
use crate::wasi::cli::{stderr, terminal_stderr, terminal_stdin, terminal_stdout};
use crate::wasi::filesystem::preopens;
use crate::wasi::filesystem::types as filesystem_types;
use crate::wasi::io::streams;

// these are all the subsystems which touch streams + poll
use crate::wasi::clocks::monotonic_clock;
use crate::wasi::http::outgoing_handler;
use crate::wasi::http::types as http_types;
use crate::wasi::io::poll;
use crate::wasi::sockets::ip_name_lookup;
use crate::wasi::sockets::tcp;
use crate::wasi::sockets::udp;

use crate::VirtAdapter;

// for debugging build
const DEBUG: bool = cfg!(feature = "debug");

use std::alloc::Layout;
use std::cell::Cell;
use std::cmp;
use std::collections::BTreeMap;
use std::ffi::CStr;
use std::rc::Rc;
use std::slice;

use wit_bindgen::Resource;

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
            log(&format!($dst));
        }
    };
}

fn log(msg: &str) {
    if unsafe { &STATE.host_stderr }.is_none() {
        unsafe { STATE.host_stderr = Some(stderr::get_stderr()) };
    }

    let msg = format!("{msg}\n");
    let _ = unsafe { &STATE.host_stderr }
        .as_ref()
        .unwrap()
        .blocking_write_and_flush(msg.as_bytes());
}

#[derive(Debug)]
pub enum IoError {
    FsCode(ErrorCode),
    Host(streams::Error),
}

#[derive(Debug)]
pub enum IoInputStream {
    Null,
    Err,
    StaticFile {
        entry: &'static StaticIndexEntry,
        offset: Cell<u64>,
    },
    Host(streams::InputStream),
}

#[derive(Debug)]
pub enum IoOutputStream {
    Null,
    Err,
    Host(streams::OutputStream),
}

#[derive(Debug)]
pub enum IoPollable {
    Null,
    Host(poll::Pollable),
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

#[no_mangle]
pub static mut io: Io = Io {
    preopen_cnt: 0,                             // [byte 0]
    preopens: 0 as *const usize,                // [byte 4]
    static_index_cnt: 0,                        // [byte 8]
    static_index: 0 as *const StaticIndexEntry, // [byte 12]
    flags: 0,                                   // [byte 16]
};

#[derive(Debug)]
#[repr(C)]
pub struct StaticIndexEntry {
    name: *const i8,
    ty: StaticIndexType,
    data: StaticFileData,
}

impl StaticIndexEntry {
    fn next(&self, idx: &Cell<usize>) -> Result<Option<DirectoryEntry>, ErrorCode> {
        let child_list = self.child_list()?;
        let child = if idx.get() < child_list.len() {
            let child = &child_list[idx.get()];
            Some(DirectoryEntry {
                type_: child.ty(),
                name: child.name().into(),
            })
        } else {
            None
        };
        idx.set(idx.get() + 1);
        Ok(child)
    }
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
                let stat = fd
                    .stat_at(filesystem_types::PathFlags::empty(), subpath)
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
    fn read(&self, offset: &Cell<u64>, len: u64) -> Result<Vec<u8>, StreamError> {
        match self.ty {
            StaticIndexType::ActiveFile => {
                if offset.get() as usize == unsafe { self.data.active.1 } {
                    return Err(StreamError::Closed);
                }
                if offset.get() as usize > unsafe { self.data.active.1 } {
                    return Err(StreamError::LastOperationFailed(Resource::new(
                        StreamsError::FsCode(ErrorCode::InvalidSeek),
                    )));
                }
                let read_ptr = unsafe { self.data.active.0.add(offset.get() as usize) };
                let read_len = cmp::min(
                    unsafe { self.data.active.1 } - offset.get() as usize,
                    len as usize,
                );
                let bytes = unsafe { slice::from_raw_parts(read_ptr, read_len) };
                offset.set(offset.get() + read_len as u64);
                Ok(bytes.to_vec())
            }
            StaticIndexType::PassiveFile => {
                if offset.get() as usize >= unsafe { self.data.passive.1 } {
                    return Err(StreamError::Closed);
                }
                if offset.get() as usize > unsafe { self.data.passive.1 } {
                    return Err(StreamError::LastOperationFailed(Resource::new(
                        StreamsError::FsCode(ErrorCode::InvalidSeek),
                    )));
                }
                let read_len = cmp::min(
                    unsafe { self.data.passive.1 } - offset.get() as usize,
                    len as usize,
                );
                let data = passive_alloc(
                    unsafe { self.data.passive.0 },
                    offset.get() as u32,
                    read_len as u32,
                );
                let bytes = unsafe { slice::from_raw_parts(data, read_len) };
                let vec = bytes.to_vec();
                unsafe { std::alloc::dealloc(data, Layout::from_size_align(1, 4).unwrap()) };
                offset.set(offset.get() + read_len as u64);
                Ok(vec)
            }
            StaticIndexType::RuntimeDir | StaticIndexType::Dir => {
                Err(StreamError::LastOperationFailed(Resource::new(
                    StreamsError::FsCode(ErrorCode::IsDirectory),
                )))
            }
            StaticIndexType::RuntimeFile => {
                // log("Internal error: Runtime file should not be reflected directly on descriptors");
                unreachable!();
            }
        }
    }
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

impl std::fmt::Debug for StaticFileData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!(
            "STATIC [{:?}, {:?}]",
            unsafe { self.dir.0 },
            unsafe { self.dir.1 }
        ))?;
        Ok(())
    }
}

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

#[derive(Debug, Clone)]
pub enum FilesystemDescriptor {
    Static(&'static StaticIndexEntry),
    Host(Rc<filesystem_types::Descriptor>),
}

impl FilesystemDescriptor {
    fn get_type(&self) -> Result<DescriptorType, ErrorCode> {
        match self {
            Self::Static(entry) => Ok(entry.ty()),
            Self::Host(fd) => fd.get_type().map(descriptor_ty_map).map_err(err_map),
        }
    }
}

#[derive(Debug)]
pub enum FilesystemDirectoryEntryStream {
    Static {
        entry: &'static StaticIndexEntry,
        idx: Cell<usize>,
    },
    Host(filesystem_types::DirectoryEntryStream),
}

pub struct CliTerminalInput(terminal_input::TerminalInput);
pub struct CliTerminalOutput(terminal_output::TerminalOutput);

pub struct HttpTypes;

pub struct HttpFields(http_types::Fields);
pub struct HttpFutureIncomingResponse(http_types::FutureIncomingResponse);
pub struct HttpFutureTrailers(http_types::FutureTrailers);
pub struct HttpIncomingBody(http_types::IncomingBody);
pub struct HttpIncomingRequest(http_types::IncomingRequest);
pub struct HttpIncomingResponse(http_types::IncomingResponse);
pub struct HttpOutgoingBody(http_types::OutgoingBody);
pub struct HttpOutgoingRequest(http_types::OutgoingRequest);
pub struct HttpOutgoingResponse(http_types::OutgoingResponse);
pub struct HttpResponseOutparam(http_types::ResponseOutparam);

pub struct HttpRequestOptions(http_types::RequestOptions);
pub struct SocketsResolveAddressStream(ip_name_lookup::ResolveAddressStream);
pub struct SocketsTcpSocket(tcp::TcpSocket);
pub struct SocketsUdpSocket(udp::UdpSocket);
pub struct SocketsIncomingDatagramStream(udp::IncomingDatagramStream);
pub struct SocketsOutgoingDatagramStream(udp::OutgoingDatagramStream);

pub struct IoState {
    initialized: bool,
    preopen_directories: Vec<(Descriptor, String)>,
    host_preopen_directories: BTreeMap<String, Rc<filesystem_types::Descriptor>>,
    host_stderr: Option<streams::OutputStream>,
}

impl IoState {
    fn initialize() {
        if unsafe { STATE.initialized } {
            return;
        }

        if DEBUG {
            std::panic::set_hook(Box::new(|invoke| {
                debug!("{:?}", invoke);
            }));
        }

        if Io::host_passthrough() || Io::host_preopens() {
            let host_preopen_directories = unsafe { &mut STATE.host_preopen_directories };
            for (fd, name) in preopens::get_directories() {
                let fd = Rc::new(fd);
                if Io::host_preopens() {
                    let fd = Descriptor::Host(fd.clone());
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
            let fd = Descriptor::Static(preopen);
            let entry = (fd, preopen.name().to_string());
            unsafe { STATE.preopen_directories.push(entry) }
        }

        unsafe { STATE.initialized = true };
    }

    fn get_host_preopen<'a>(
        path: &'a str,
    ) -> Option<(&'static filesystem_types::Descriptor, &'a str)> {
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
                        return Some((fd, &path));
                    }
                } else {
                    // root '/' match
                    if preopen_name == "/" && path.as_bytes()[0] == b'/' {
                        return Some((fd, &path[1..]));
                    }
                    // exact match
                    if preopen_name.len() == path.len() {
                        return Some((fd, ""));
                    }
                    // normal [x]/ match
                    if path.as_bytes()[preopen_name.len()] == b'/' {
                        return Some((fd, &path[preopen_name.len() + 1..]));
                    }
                }
            }
        }
        None
    }
}

static mut STATE: IoState = IoState {
    initialized: false,
    preopen_directories: Vec::new(),
    host_preopen_directories: BTreeMap::new(),
    host_stderr: None,
};

impl Stdin for VirtAdapter {
    fn get_stdin() -> Resource<InputStream> {
        debug!("CALL wasi:cli/stdin#get-stdin");
        Resource::new(match Io::stdin() {
            AllowCfg::Allow => InputStream::Host(stdin::get_stdin()),
            AllowCfg::Ignore => InputStream::Null,
            AllowCfg::Deny => InputStream::Err,
        })
    }
}

impl Stdout for VirtAdapter {
    fn get_stdout() -> Resource<OutputStream> {
        debug!("CALL wasi:cli/stdout#get-stdout");
        Resource::new(match Io::stdout() {
            AllowCfg::Allow => OutputStream::Host(stdout::get_stdout()),
            AllowCfg::Ignore => OutputStream::Null,
            AllowCfg::Deny => OutputStream::Err,
        })
    }
}

impl Stderr for VirtAdapter {
    fn get_stderr() -> Resource<OutputStream> {
        debug!("CALL wasi:cli/stderr#get-stderr");
        Resource::new(match Io::stderr() {
            AllowCfg::Allow => OutputStream::Host(stderr::get_stderr()),
            AllowCfg::Ignore => OutputStream::Null,
            AllowCfg::Deny => OutputStream::Err,
        })
    }
}

impl TerminalStdin for VirtAdapter {
    fn get_terminal_stdin() -> Option<Resource<TerminalInput>> {
        debug!("CALL wasi:cli/terminal-stdin#get-terminal-stdin");
        terminal_stdin::get_terminal_stdin()
            .map(|terminal_input| Resource::new(CliTerminalInput(terminal_input)))
    }
}

impl TerminalStdout for VirtAdapter {
    fn get_terminal_stdout() -> Option<Resource<TerminalOutput>> {
        debug!("CALL wasi:cli/terminal-stdout#get-terminal-stdout");
        terminal_stdout::get_terminal_stdout()
            .map(|terminal_output| Resource::new(CliTerminalOutput(terminal_output)))
    }
}

impl TerminalStderr for VirtAdapter {
    fn get_terminal_stderr() -> Option<Resource<TerminalOutput>> {
        debug!("CALL wasi:cli/terminal-stderr#get-terminal-stderr");
        terminal_stderr::get_terminal_stderr()
            .map(|terminal_output| Resource::new(CliTerminalOutput(terminal_output)))
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
    fn subscribe_instant(when: u64) -> Resource<Pollable> {
        debug!("CALL wasi:clocks/monotonic-clock#subscribe-instant");
        let host_pollable = monotonic_clock::subscribe_instant(when);
        Resource::new(Pollable::Host(host_pollable))
    }
    fn subscribe_duration(when: u64) -> Resource<Pollable> {
        debug!("CALL wasi:clocks/monotonic-clock#subscribe-duration");
        let host_pollable = monotonic_clock::subscribe_duration(when);
        Resource::new(Pollable::Host(host_pollable))
    }
}

impl FilesystemTypes for VirtAdapter {
    fn filesystem_error_code(err: &StreamsError) -> Option<ErrorCode> {
        debug!("CALL wasi:filesystem/types#filesystem-error-code");
        if let StreamsError::FsCode(code) = err {
            Some(*code)
        } else {
            None
        }
    }
}

impl Preopens for VirtAdapter {
    fn get_directories() -> Vec<(Resource<Descriptor>, String)> {
        debug!("CALL wasi:filesystem/preopens#get-directories");
        IoState::initialize();
        unsafe { &STATE.preopen_directories }
            .iter()
            .map(|(fd, name)| (Resource::new(fd.clone()), name.clone()))
            .collect()
    }
}

impl OutgoingHandler for VirtAdapter {
    fn handle(
        request: Resource<OutgoingRequest>,
        options: Option<Resource<RequestOptions>>,
    ) -> Result<Resource<FutureIncomingResponse>, HttpErrorCode> {
        outgoing_handler::handle(
            Resource::into_inner(request).0,
            options.map(|o| Resource::into_inner(o).0),
        )
        .map(|response| Resource::new(HttpFutureIncomingResponse(response)))
        .map_err(http_err_map_rev)
    }
}

impl GuestRequestOptions for RequestOptions {
    fn new() -> Self {
        debug!("CALL wasi:http/types#request-options.new");
        Self(http_types::RequestOptions::new())
    }
    fn connect_timeout(&self) -> Option<Duration> {
        debug!("CALL wasi:http/types#request-options.connect-timeout");
        self.0.connect_timeout()
    }
    fn set_connect_timeout(&self, duration: Option<Duration>) -> Result<(), ()> {
        debug!("CALL wasi:http/types#request-options.set-connect-timeout");
        self.0.set_connect_timeout(duration)
    }
    fn first_byte_timeout(&self) -> Option<Duration> {
        debug!("CALL wasi:http/types#request-options.first-byte-timeout");
        self.0.first_byte_timeout()
    }
    fn set_first_byte_timeout(&self, duration: Option<Duration>) -> Result<(), ()> {
        debug!("CALL wasi:http/types#request-options.set-first-byte-timeout");
        self.0.set_first_byte_timeout(duration)
    }
    fn between_bytes_timeout(&self) -> Option<Duration> {
        debug!("CALL wasi:http/types#request-options.between-bytes-timeout");
        self.0.between_bytes_timeout()
    }
    fn set_between_bytes_timeout(&self, duration: Option<Duration>) -> Result<(), ()> {
        debug!("CALL wasi:http/types#request-options.set-between-bytes-timeout");
        self.0.set_between_bytes_timeout(duration)
    }
}

impl GuestPollable for IoPollable {
    fn ready(&self) -> bool {
        debug!("CALL wasi:io/poll#pollable.ready PID={self:?}",);
        match self {
            IoPollable::Host(pid) => pid.ready(),
            IoPollable::Null => true,
        }
    }

    fn block(&self) {
        debug!("CALL wasi:io/poll#pollable.block PID={self:?}",);
        match self {
            IoPollable::Host(pid) => pid.block(),
            IoPollable::Null => (),
        }
    }
}

impl Poll for VirtAdapter {
    fn poll(list: Vec<&Pollable>) -> Vec<u32> {
        debug!("CALL wasi:io/poll#poll-list PIDS={list:?}",);
        let has_host_polls = list.iter().any(|&pid| matches!(pid, Pollable::Host(_)));
        let has_virt_polls = list.iter().any(|&pid| matches!(pid, Pollable::Null));
        if has_host_polls && !has_virt_polls {
            return poll::poll(
                &list
                    .iter()
                    .map(|&pid| {
                        if let Pollable::Host(pid) = pid {
                            pid
                        } else {
                            unreachable!()
                        }
                    })
                    .collect::<Vec<_>>(),
            );
        }
        if has_virt_polls {
            return (0..list.len()).map(|n| n.try_into().unwrap()).collect();
        }
        let mut host_polls = Vec::new();
        let mut host_map = Vec::new();
        for (index, pid) in list.iter().enumerate() {
            if let Pollable::Host(host_pid) = pid {
                host_polls.push(host_pid);
                host_map.push(u32::try_from(index).unwrap());
            }
        }
        let mut ready = poll::poll(&host_polls)
            .into_iter()
            .map(|index| host_map[usize::try_from(index).unwrap()])
            .collect::<Vec<_>>();
        for (index, pid) in list.iter().enumerate() {
            if let Pollable::Null = pid {
                ready.push(index.try_into().unwrap());
            }
        }
        ready
    }
}

impl IpNameLookup for VirtAdapter {
    fn resolve_addresses(
        network: &Network,
        name: String,
    ) -> Result<Resource<ResolveAddressStream>, NetworkErrorCode> {
        debug!("CALL wasi:sockets/ip-name-lookup#resolve-addresses");
        Ok(Resource::new(SocketsResolveAddressStream(
            ip_name_lookup::resolve_addresses(network, &name)?,
        )))
    }
}

impl GuestDescriptor for Descriptor {
    fn read_via_stream(&self, offset: u64) -> Result<Resource<InputStream>, ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.read-via-stream FD={self:?} OFFSET={offset}",);
        Ok(Resource::new(match self {
            Self::Static(entry) => InputStream::StaticFile {
                entry,
                offset: Cell::new(offset),
            },
            Self::Host(descriptor) => {
                InputStream::Host(descriptor.read_via_stream(offset).map_err(err_map)?)
            }
        }))
    }
    fn write_via_stream(&self, offset: u64) -> Result<Resource<OutputStream>, ErrorCode> {
        debug!(
            "CALL wasi:filesystem/types#descriptor.write-via-stream FD={self:?} OFFSET={offset}",
        );
        Err(ErrorCode::Access)
    }
    fn append_via_stream(&self) -> Result<Resource<OutputStream>, ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.append-via-stream FD={self:?}");
        Err(ErrorCode::Access)
    }
    fn advise(&self, _: u64, _: u64, _: Advice) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.advise FD={self:?}");
        todo!()
    }
    fn sync_data(&self) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.sync-data FD={self:?}");
        Err(ErrorCode::Access)
    }
    fn get_flags(&self) -> Result<DescriptorFlags, ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.get-flags FD={self:?}");
        Ok(DescriptorFlags::READ)
    }
    fn get_type(&self) -> Result<DescriptorType, ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.get-type FD={self:?}");
        self.get_type()
    }
    fn set_size(&self, _: u64) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.set-size FD={self:?}");
        Err(ErrorCode::Access)
    }
    fn set_times(&self, _: NewTimestamp, _: NewTimestamp) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.set-times FD={self:?}");
        Err(ErrorCode::Access)
    }
    fn read(&self, len: u64, offset: u64) -> Result<(Vec<u8>, bool), ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.read FD={self:?}");
        match self.read_via_stream(offset)?.read(len) {
            Ok(bytes) => Ok((bytes, false)),
            Err(StreamError::Closed) => Ok((Vec::new(), true)),
            Err(StreamError::LastOperationFailed(_)) => Err(ErrorCode::Io),
        }
    }
    fn write(&self, _: Vec<u8>, _: u64) -> Result<u64, ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.write FD={self:?}");
        Err(ErrorCode::Access)
    }
    fn read_directory(&self) -> Result<Resource<DirectoryEntryStream>, ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.read-directory FD={self:?}");
        if self.get_type()? != DescriptorType::Directory {
            return Err(ErrorCode::NotDirectory);
        }
        Ok(Resource::new(match self {
            Self::Static(entry) => DirectoryEntryStream::Static {
                entry,
                idx: Cell::new(0),
            },
            Self::Host(descriptor) => {
                DirectoryEntryStream::Host(descriptor.read_directory().map_err(err_map)?)
            }
        }))
    }
    fn sync(&self) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.sync FD={self:?}");
        Err(ErrorCode::Access)
    }
    fn create_directory_at(&self, path: String) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.create-directory-at FD={self:?} PATH={path}",);
        Err(ErrorCode::Access)
    }
    fn stat(&self) -> Result<DescriptorStat, ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.stat FD={self:?}");
        match self {
            Self::Static(entry) => Ok(DescriptorStat {
                type_: entry.ty(),
                link_count: 0,
                size: entry.size()?,
                data_access_timestamp: None,
                data_modification_timestamp: None,
                status_change_timestamp: None,
            }),
            Self::Host(descriptor) => descriptor.stat().map(stat_map).map_err(err_map),
        }
    }
    fn stat_at(&self, flags: PathFlags, path: String) -> Result<DescriptorStat, ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.stat-at FD={self:?} PATH={path}");
        match self {
            Self::Static(entry) => {
                let child = entry.dir_lookup(&path)?;
                if matches!(
                    child.ty,
                    StaticIndexType::RuntimeDir | StaticIndexType::RuntimeFile
                ) {
                    let Some((host_fd, path)) = IoState::get_host_preopen(child.runtime_path())
                    else {
                        return Err(ErrorCode::NoEntry);
                    };
                    host_fd
                        .stat_at(
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
                        data_access_timestamp: None,
                        data_modification_timestamp: None,
                        status_change_timestamp: None,
                    })
                }
            }
            Self::Host(host_fd) => host_fd
                .stat_at(
                    filesystem_types::PathFlags::from_bits(flags.bits()).unwrap(),
                    &path,
                )
                .map(stat_map)
                .map_err(err_map),
        }
    }
    fn set_times_at(
        &self,
        _: PathFlags,
        path: String,
        _: NewTimestamp,
        _: NewTimestamp,
    ) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.set-times-at FD={self:?} PATH={path}",);
        Err(ErrorCode::Access)
    }
    fn link_at(&self, _: PathFlags, path: String, _: &Self, _: String) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.link-at FD={self:?} PATH={path}",);
        Err(ErrorCode::Access)
    }
    fn open_at(
        &self,
        path_flags: PathFlags,
        path: String,
        open_flags: OpenFlags,
        descriptor_flags: DescriptorFlags,
    ) -> Result<Resource<Self>, ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.open-at FD={self:?} PATH={path}",);
        match self {
            Self::Static(entry) => {
                let child = entry.dir_lookup(&path)?;
                if matches!(
                    child.ty,
                    StaticIndexType::RuntimeDir | StaticIndexType::RuntimeFile
                ) {
                    let Some((host_fd, path)) = IoState::get_host_preopen(child.runtime_path())
                    else {
                        return Err(ErrorCode::NoEntry);
                    };
                    let child_fd = host_fd
                        .open_at(
                            filesystem_types::PathFlags::from_bits(path_flags.bits()).unwrap(),
                            &path,
                            filesystem_types::OpenFlags::from_bits(open_flags.bits()).unwrap(),
                            filesystem_types::DescriptorFlags::from_bits(descriptor_flags.bits())
                                .unwrap(),
                        )
                        .map_err(err_map)?;
                    Ok(Resource::new(Self::Host(Rc::new(child_fd))))
                } else {
                    Ok(Resource::new(Self::Static(child)))
                }
            }
            Self::Host(host_fd) => {
                let child_fd = host_fd
                    .open_at(
                        filesystem_types::PathFlags::from_bits(path_flags.bits()).unwrap(),
                        &path,
                        filesystem_types::OpenFlags::from_bits(open_flags.bits()).unwrap(),
                        filesystem_types::DescriptorFlags::from_bits(descriptor_flags.bits())
                            .unwrap(),
                    )
                    .map_err(err_map)?;
                Ok(Resource::new(Self::Host(Rc::new(child_fd))))
            }
        }
    }
    fn readlink_at(&self, path: String) -> Result<String, ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.readlink-at FD={self:?} PATH={path}",);
        match self {
            Self::Static(entry) => {
                let child = entry.dir_lookup(&path)?;
                if matches!(
                    child.ty,
                    StaticIndexType::RuntimeDir | StaticIndexType::RuntimeFile
                ) {
                    let Some((host_fd, path)) = IoState::get_host_preopen(child.runtime_path())
                    else {
                        return Err(ErrorCode::NoEntry);
                    };
                    host_fd.readlink_at(&path).map_err(err_map)
                } else {
                    Err(ErrorCode::Invalid)
                }
            }
            Self::Host(host_fd) => host_fd.readlink_at(&path).map_err(err_map),
        }
    }
    fn remove_directory_at(&self, path: String) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.remove-directory-at FD={self:?} PATH={path}",);
        Err(ErrorCode::Access)
    }
    fn rename_at(&self, path: String, _: &Self, _: String) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.rename-at FD={self:?} PATH={path}",);
        Err(ErrorCode::Access)
    }
    fn symlink_at(&self, path: String, _: String) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.symlink-at FD={self:?} PATH={path}",);
        Err(ErrorCode::Access)
    }
    fn unlink_file_at(&self, path: String) -> Result<(), ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.unlink-file-at FD={self:?} PATH={path}",);
        Err(ErrorCode::Access)
    }
    fn is_same_object(&self, other: &Self) -> bool {
        debug!("CALL wasi:filesystem/types#descriptor.is-same-object FD1={self:?} FD2={other:?}",);
        // already-opened static index descriptors will never point to a RuntimeFile
        // or RuntimeDir - instead they point to an already-created HostDescriptor
        match (self, other) {
            (Self::Static(entry1), Self::Static(entry2)) => {
                entry1 as *const _ == entry2 as *const _
            }
            (Self::Host(host_fd1), Self::Host(host_fd2)) => host_fd1.is_same_object(host_fd2),
            _ => false,
        }
    }
    fn metadata_hash(&self) -> Result<MetadataHashValue, ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.metadata-hash FD={self:?}");
        match self {
            Self::Static(entry) => Ok(MetadataHashValue {
                upper: entry.idx() as u64,
                lower: 0,
            }),
            Self::Host(host_fd) => host_fd
                .metadata_hash()
                .map(metadata_hash_map)
                .map_err(err_map),
        }
    }
    fn metadata_hash_at(
        &self,
        path_flags: PathFlags,
        path: String,
    ) -> Result<MetadataHashValue, ErrorCode> {
        debug!("CALL wasi:filesystem/types#descriptor.metadata-hash-at FD={self:?} PATH={path}",);
        match self {
            Self::Static(entry) => {
                let child = entry.dir_lookup(&path)?;
                if matches!(
                    child.ty,
                    StaticIndexType::RuntimeDir | StaticIndexType::RuntimeFile
                ) {
                    let Some((host_fd, path)) = IoState::get_host_preopen(child.runtime_path())
                    else {
                        return Err(ErrorCode::NoEntry);
                    };
                    host_fd
                        .metadata_hash_at(
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
            Self::Host(host_fd) => host_fd
                .metadata_hash_at(
                    filesystem_types::PathFlags::from_bits(path_flags.bits()).unwrap(),
                    &path,
                )
                .map(metadata_hash_map)
                .map_err(err_map),
        }
    }
}

impl GuestInputStream for InputStream {
    fn read(&self, len: u64) -> Result<Vec<u8>, StreamError> {
        debug!("CALL wasi:io/streams#input-stream.read SID={self:?}");
        match self {
            Self::Null => Ok(Vec::new()),
            Self::Err => Err(StreamError::Closed),
            Self::StaticFile { .. } => self.blocking_read(len),
            Self::Host(descriptor) => descriptor.read(len).map_err(stream_err_map),
        }
    }
    fn blocking_read(&self, len: u64) -> Result<Vec<u8>, StreamError> {
        debug!("CALL wasi:io/streams#input-stream.blocking-read SID={self:?}");
        match self {
            Self::Null => Ok(Vec::new()),
            Self::Err => Err(StreamError::Closed),
            Self::StaticFile { entry, offset } => entry.read(offset, len),
            Self::Host(descriptor) => descriptor.blocking_read(len).map_err(stream_err_map),
        }
    }
    fn skip(&self, offset: u64) -> Result<u64, StreamError> {
        debug!("CALL wasi:io/streams#input-stream.skip SID={self:?}");
        match self {
            Self::Null => Ok(0),
            Self::Err => Err(StreamError::Closed),
            Self::StaticFile { .. } => Err(StreamError::LastOperationFailed(Resource::new(
                StreamsError::FsCode(ErrorCode::Io),
            ))),
            Self::Host(descriptor) => descriptor.skip(offset).map_err(stream_err_map),
        }
    }
    fn blocking_skip(&self, offset: u64) -> Result<u64, StreamError> {
        debug!("CALL wasi:io/streams#input-stream.blocking-skip SID={self:?}");
        match self {
            Self::Null => Ok(0),
            Self::Err => Err(StreamError::Closed),
            Self::StaticFile { .. } => Err(StreamError::LastOperationFailed(Resource::new(
                StreamsError::FsCode(ErrorCode::Io),
            ))),
            Self::Host(descriptor) => descriptor.blocking_skip(offset).map_err(stream_err_map),
        }
    }
    fn subscribe(&self) -> Resource<Pollable> {
        debug!("CALL wasi:io/streams#input-stream.subscribe SID={self:?}");
        Resource::new(match self {
            Self::Null | Self::Err | Self::StaticFile { .. } => Pollable::Null,
            Self::Host(descriptor) => Pollable::Host(descriptor.subscribe()),
        })
    }
}

impl GuestOutputStream for OutputStream {
    fn check_write(&self) -> Result<u64, StreamError> {
        debug!("CALL wasi:io/streams#output-stream.check_write SID={self:?}");
        match self {
            Self::Null => Ok(1024 * 1024),
            Self::Err => Err(StreamError::Closed),
            Self::Host(sid) => sid.check_write().map_err(stream_err_map),
        }
    }
    fn write(&self, bytes: Vec<u8>) -> Result<(), StreamError> {
        debug!("CALL wasi:io/streams#output-stream.write SID={self:?}");
        match self {
            Self::Null => Ok(()),
            Self::Err => Err(StreamError::Closed),
            Self::Host(sid) => sid.write(&bytes).map_err(stream_err_map),
        }
    }
    fn blocking_write_and_flush(&self, bytes: Vec<u8>) -> Result<(), StreamError> {
        debug!("CALL wasi:io/streams#output-stream.blocking-write-and-flush SID={self:?}");
        match self {
            Self::Null => Ok(()),
            Self::Err => Err(StreamError::Closed),
            Self::Host(sid) => sid.blocking_write_and_flush(&bytes).map_err(stream_err_map),
        }
    }
    fn flush(&self) -> Result<(), StreamError> {
        debug!("CALL wasi:io/streams#output-stream.flush SID={self:?}");
        match self {
            Self::Null => Ok(()),
            Self::Err => Err(StreamError::Closed),
            Self::Host(sid) => sid.flush().map_err(stream_err_map),
        }
    }
    fn blocking_flush(&self) -> Result<(), StreamError> {
        debug!("CALL wasi:io/streams#output-stream.blocking-flush SID={self:?}");
        match self {
            Self::Null => Ok(()),
            Self::Err => Err(StreamError::Closed),
            Self::Host(sid) => sid.blocking_flush().map_err(stream_err_map),
        }
    }
    fn write_zeroes(&self, len: u64) -> Result<(), StreamError> {
        debug!("CALL wasi:io/streams#output-stream.write-zeroes SID={self:?}");
        match self {
            Self::Null => Ok(()),
            Self::Err => Err(StreamError::Closed),
            Self::Host(sid) => sid.write_zeroes(len).map_err(stream_err_map),
        }
    }
    fn blocking_write_zeroes_and_flush(&self, len: u64) -> Result<(), StreamError> {
        debug!("CALL wasi:io/streams#output-stream.blocking-write-zeroes-and-flush SID={self:?}");
        match self {
            Self::Null => Ok(()),
            Self::Err => Err(StreamError::Closed),
            Self::Host(sid) => sid
                .blocking_write_zeroes_and_flush(len)
                .map_err(stream_err_map),
        }
    }
    fn splice(&self, from: &InputStream, len: u64) -> Result<u64, StreamError> {
        debug!("CALL wasi:io/streams#output-stream.splice TO_SID={self:?} FROM_SID={from:?}",);
        let to_sid = match self {
            Self::Null => {
                return Ok(len);
            }
            Self::Err => {
                return Err(StreamError::Closed);
            }
            Self::Host(sid) => sid,
        };
        let from_sid = match from {
            InputStream::Null => {
                return Ok(len);
            }
            InputStream::Err => {
                return Err(StreamError::Closed);
            }
            InputStream::StaticFile { .. } => todo!(),
            InputStream::Host(sid) => sid,
        };
        to_sid.splice(&from_sid, len).map_err(stream_err_map)
    }
    fn blocking_splice(&self, from: &IoInputStream, len: u64) -> Result<u64, StreamError> {
        debug!(
            "CALL wasi:io/streams#output-stream.blocking-splice TO_SID={self:?} FROM_SID={from:?}",
        );
        let to_sid = match self {
            Self::Null => {
                return Ok(len);
            }
            Self::Err => {
                return Err(StreamError::Closed);
            }
            Self::Host(sid) => sid,
        };
        let from_sid = match from {
            InputStream::Null => {
                return Ok(len);
            }
            InputStream::Err => {
                return Err(StreamError::Closed);
            }
            InputStream::StaticFile { .. } => todo!(),
            InputStream::Host(sid) => sid,
        };
        to_sid
            .blocking_splice(&from_sid, len)
            .map_err(stream_err_map)
    }
    fn subscribe(&self) -> Resource<Pollable> {
        debug!("CALL wasi:io/streams#output-stream.subscribe SID={self:?}");
        Resource::new(match self {
            Self::Null | Self::Err => Pollable::Null,
            Self::Host(descriptor) => Pollable::Host(descriptor.subscribe()),
        })
    }
}

impl GuestStreamsError for StreamsError {
    fn to_debug_string(&self) -> String {
        debug!("CALL wasi:io/error#to-debug-string");
        format!("{self:?}")
    }
}

impl GuestDirectoryEntryStream for DirectoryEntryStream {
    fn read_directory_entry(&self) -> Result<Option<DirectoryEntry>, ErrorCode> {
        debug!("CALL wasi:filesystem/types#read-directory-entry SID={self:?}");
        match self {
            Self::Static { entry, idx } => entry.next(idx),
            Self::Host(sid) => sid
                .read_directory_entry()
                .map(|e| e.map(dir_map))
                .map_err(err_map),
        }
    }
}

impl GuestHttpTypes for HttpTypes {
    fn http_error_code(err: &IoError) -> Option<HttpErrorCode> {
        debug!("CALL wasi:http/types#http-error-code");
        match err {
            IoError::FsCode(_) => None,
            IoError::Host(h) => http_types::http_error_code(h).map(|e| http_err_map_rev(e)),
        }
    }
}

impl GuestFields for Fields {
    fn new() -> Self {
        debug!("CALL wasi:http/types#fields.constructor");
        Self(http_types::Fields::new())
    }

    fn get(&self, name: String) -> Vec<Vec<u8>> {
        debug!("CALL wasi:http/types#fields.get NAME={name}");
        self.0.get(&name)
    }

    fn set(&self, name: String, values: Vec<Vec<u8>>) -> Result<(), HeaderError> {
        debug!("CALL wasi:http/types#fields.set NAME={name}");
        self.0.set(&name, &values).map_err(header_err_map_rev)
    }

    fn delete(&self, name: String) -> Result<(), HeaderError> {
        debug!("CALL wasi:http/types#fields.delete NAME={name}");
        self.0.delete(&name).map_err(header_err_map_rev)
    }

    fn append(&self, name: String, value: Vec<u8>) -> Result<(), HeaderError> {
        debug!("CALL wasi:http/types#fields.append NAME={name}");
        self.0.append(&name, &value).map_err(header_err_map_rev)
    }

    fn entries(&self) -> Vec<(String, Vec<u8>)> {
        debug!("CALL wasi:http/types#fields.entries");
        self.0.entries()
    }

    fn clone(&self) -> Resource<Self> {
        debug!("CALL wasi:http/types#fields.clone");
        Resource::new(Self(self.0.clone()))
    }

    fn from_list(list: Vec<(String, Vec<u8>)>) -> Result<Resource<HttpFields>, HeaderError> {
        debug!("CALL wasi:http/types#fields.from-list");
        http_types::Fields::from_list(&list)
            .map(|fields| Resource::new(Self(fields)))
            .map_err(header_err_map_rev)
    }

    fn has(&self, name: String) -> bool {
        debug!("CALL wasi:http/types#fields.has NAME={name}");
        self.0.has(&name)
    }
}

impl GuestIncomingRequest for IncomingRequest {
    fn method(&self) -> Method {
        debug!("CALL wasi:http/types#incoming-request.method");
        method_map_rev(self.0.method())
    }
    fn path_with_query(&self) -> Option<String> {
        debug!("CALL wasi:http/types#incoming-request.path-with-query");
        self.0.path_with_query()
    }
    fn scheme(&self) -> Option<Scheme> {
        debug!("CALL wasi:http/types#incoming-request.scheme");
        self.0.scheme().map(scheme_map_rev)
    }
    fn authority(&self) -> Option<String> {
        debug!("CALL wasi:http/types#incoming-request.authority");
        self.0.authority()
    }
    fn headers(&self) -> Resource<Fields> {
        debug!("CALL wasi:http/types#incoming-request.headers");
        Resource::new(HttpFields(self.0.headers()))
    }
    fn consume(&self) -> Result<Resource<IncomingBody>, ()> {
        debug!("CALL wasi:http/types#incoming-request.consume");
        Ok(Resource::new(HttpIncomingBody(self.0.consume()?)))
    }
}

impl GuestOutgoingRequest for OutgoingRequest {
    fn new(headers: Resource<Headers>) -> Self {
        debug!("CALL wasi:http/types#outgoing-request.new");
        Self(http_types::OutgoingRequest::new(
            Resource::into_inner(headers).0,
        ))
    }

    fn body(&self) -> Result<Resource<OutgoingBody>, ()> {
        debug!("CALL wasi:http/types#outgoing-request.write");
        Ok(Resource::new(HttpOutgoingBody(self.0.body()?)))
    }

    fn method(&self) -> Method {
        method_map_rev(self.0.method())
    }

    fn set_method(&self, method: Method) -> Result<(), ()> {
        self.0.set_method(&method_map(method))
    }

    fn path_with_query(&self) -> Option<String> {
        self.0.path_with_query()
    }

    fn set_path_with_query(&self, path_with_query: Option<String>) -> Result<(), ()> {
        self.0.set_path_with_query(path_with_query.as_deref())
    }

    fn scheme(&self) -> Option<Scheme> {
        self.0.scheme().map(scheme_map_rev)
    }

    fn set_scheme(&self, scheme: Option<Scheme>) -> Result<(), ()> {
        self.0.set_scheme(scheme.map(scheme_map).as_ref())
    }

    fn authority(&self) -> Option<String> {
        self.0.authority()
    }

    fn set_authority(&self, authority: Option<String>) -> Result<(), ()> {
        self.0.set_authority(authority.as_deref())
    }

    fn headers(&self) -> Resource<Headers> {
        Resource::new(HttpFields(self.0.headers()))
    }
}

impl GuestResponseOutparam for ResponseOutparam {
    fn set(param: Resource<Self>, response: Result<Resource<OutgoingResponse>, HttpErrorCode>) {
        debug!("CALL wasi:http/types#response-outparam.set");
        let param = Resource::into_inner(param).0;
        match response {
            Ok(res) => http_types::ResponseOutparam::set(param, Ok(Resource::into_inner(res).0)),
            Err(err) => http_types::ResponseOutparam::set(param, Err(http_err_map(err))),
        }
    }
}

impl GuestIncomingResponse for IncomingResponse {
    fn status(&self) -> StatusCode {
        debug!("CALL wasi:http/types#incoming-response.status");
        self.0.status()
    }
    fn headers(&self) -> Resource<Fields> {
        debug!("CALL wasi:http/types#incoming-response.headers");
        Resource::new(HttpFields(self.0.headers()))
    }
    fn consume(&self) -> Result<Resource<IncomingBody>, ()> {
        debug!("CALL wasi:http/types#incoming-response.consume");
        Ok(Resource::new(HttpIncomingBody(self.0.consume()?)))
    }
}

impl GuestIncomingBody for IncomingBody {
    fn stream(&self) -> Result<Resource<InputStream>, ()> {
        debug!("CALL wasi:http/types#incoming-body.stream");
        Ok(Resource::new(InputStream::Host(self.0.stream()?)))
    }

    fn finish(body: Resource<IncomingBody>) -> Resource<FutureTrailers> {
        debug!("CALL wasi:http/types#incoming-body.finish");
        Resource::new(HttpFutureTrailers(http_types::IncomingBody::finish(
            Resource::into_inner(body).0,
        )))
    }
}

impl GuestFutureTrailers for FutureTrailers {
    fn subscribe(&self) -> Resource<Pollable> {
        debug!("CALL wasi:http/types#future-trailers.subscribe");
        Resource::new(Pollable::Host(self.0.subscribe()))
    }

    fn get(&self) -> Option<Result<Result<Option<Resource<Fields>>, HttpErrorCode>, ()>> {
        debug!("CALL wasi:http/types#future-trailers.get");
        self.0.get().map(|r| {
            r.map(|fields| {
                fields
                    .map(|fields| fields.map(|fields| Resource::new(HttpFields(fields))))
                    .map_err(http_err_map_rev)
            })
        })
    }
}

impl GuestOutgoingResponse for OutgoingResponse {
    fn new(headers: Resource<Fields>) -> Self {
        debug!("CALL wasi:http/types#outgoing-response.constructor");
        Self(http_types::OutgoingResponse::new(
            Resource::into_inner(headers).0,
        ))
    }

    fn status_code(&self) -> StatusCode {
        debug!("CALL wasi:http/types#outgoing-response.status-code");
        self.0.status_code()
    }

    fn set_status_code(&self, status_code: StatusCode) -> Result<(), ()> {
        debug!("CALL wasi:http/types#outgoing-response.set-status-code");
        self.0.set_status_code(status_code)
    }

    fn headers(&self) -> Resource<Headers> {
        debug!("CALL wasi:http/types#outgoing-response.headers");
        Resource::new(HttpFields(self.0.headers()))
    }

    fn body(&self) -> Result<Resource<OutgoingBody>, ()> {
        debug!("CALL wasi:http/types#outgoing-response.body");
        Ok(Resource::new(HttpOutgoingBody(self.0.body()?)))
    }
}

fn dir_map(d: filesystem_types::DirectoryEntry) -> DirectoryEntry {
    DirectoryEntry {
        type_: descriptor_ty_map(d.type_),
        name: d.name,
    }
}

impl GuestOutgoingBody for OutgoingBody {
    fn write(&self) -> Result<Resource<OutputStream>, ()> {
        debug!("CALL wasi:http/types#outgoing-body.write");
        Ok(Resource::new(OutputStream::Host(self.0.write()?)))
    }

    fn finish(
        body: Resource<OutgoingBody>,
        trailers: Option<Resource<Fields>>,
    ) -> Result<(), HttpErrorCode> {
        debug!("CALL wasi:http/types#outgoing-body.finish");
        http_types::OutgoingBody::finish(
            Resource::into_inner(body).0,
            trailers.map(|fields| Resource::into_inner(fields).0),
        )
        .map_err(http_err_map_rev)
    }
}

impl GuestFutureIncomingResponse for FutureIncomingResponse {
    fn subscribe(&self) -> Resource<Pollable> {
        debug!("CALL wasi:http/types#future-incoming-response.subscribe");
        Resource::new(Pollable::Host(self.0.subscribe()))
    }

    fn get(&self) -> Option<Result<Result<Resource<IncomingResponse>, HttpErrorCode>, ()>> {
        debug!("CALL wasi:http/types#future-incoming-response.get");
        self.0.get().map(|r| {
            r.map(|r| {
                r.map(|response| Resource::new(HttpIncomingResponse(response)))
                    .map_err(|e| http_err_map_rev(e))
            })
        })
    }
}

impl GuestResolveAddressStream for ResolveAddressStream {
    fn resolve_next_address(&self) -> Result<Option<IpAddress>, NetworkErrorCode> {
        debug!("CALL wasi:sockets/ip-name-lookup#resolve-address-stream.resolve-next-address");
        self.0.resolve_next_address()
    }
    fn subscribe(&self) -> Resource<Pollable> {
        debug!("CALL wasi:sockets/ip-name-lookup#resolve-address-stream.subscribe");
        Resource::new(Pollable::Host(self.0.subscribe()))
    }
}

impl GuestTcpSocket for TcpSocket {
    fn start_bind(
        &self,
        network: &Network,
        local_address: IpSocketAddress,
    ) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.start-bind");
        self.0.start_bind(network, local_address)
    }
    fn finish_bind(&self) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.finish-bind");
        self.0.finish_bind()
    }
    fn start_connect(
        &self,
        network: &Network,
        remote_address: IpSocketAddress,
    ) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.start-connect");
        self.0.start_connect(network, remote_address)
    }
    fn finish_connect(
        &self,
    ) -> Result<(Resource<InputStream>, Resource<OutputStream>), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.finish-connect");
        self.0.finish_connect().map(|(rx, tx)| {
            (
                Resource::new(InputStream::Host(rx)),
                Resource::new(OutputStream::Host(tx)),
            )
        })
    }
    fn start_listen(&self) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.start-listen");
        self.0.start_listen()
    }
    fn finish_listen(&self) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.finish-listen");
        self.0.finish_listen()
    }
    fn accept(
        &self,
    ) -> Result<
        (
            Resource<TcpSocket>,
            Resource<InputStream>,
            Resource<OutputStream>,
        ),
        NetworkErrorCode,
    > {
        debug!("CALL wasi:sockets/tcp#tcp-socket.accept");
        self.0.accept().map(|(s, rx, tx)| {
            (
                Resource::new(SocketsTcpSocket(s)),
                Resource::new(InputStream::Host(rx)),
                Resource::new(OutputStream::Host(tx)),
            )
        })
    }
    fn local_address(&self) -> Result<IpSocketAddress, NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.local-address");
        self.0.local_address()
    }
    fn remote_address(&self) -> Result<IpSocketAddress, NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.remote-address");
        self.0.remote_address()
    }
    fn is_listening(&self) -> bool {
        debug!("CALL wasi:sockets/tcp#tcp-socket.is-listening");
        self.0.is_listening()
    }
    fn address_family(&self) -> IpAddressFamily {
        debug!("CALL wasi:sockets/tcp#tcp-socket.address-family");
        self.0.address_family()
    }
    fn set_listen_backlog_size(&self, value: u64) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.set-listen-backlog-size");
        self.0.set_listen_backlog_size(value)
    }
    fn keep_alive_enabled(&self) -> Result<bool, NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.keep-alive-enabled");
        self.0.keep_alive_enabled()
    }
    fn set_keep_alive_enabled(&self, value: bool) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.set-keep-alive-enabled");
        self.0.set_keep_alive_enabled(value)
    }
    fn keep_alive_idle_time(&self) -> Result<Duration, NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.keep-alive-idle-time");
        self.0.keep_alive_idle_time()
    }
    fn set_keep_alive_idle_time(&self, value: Duration) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.set-keep-alive-idle-time");
        self.0.set_keep_alive_idle_time(value)
    }
    fn keep_alive_interval(&self) -> Result<Duration, NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.keep-alive-interval-time");
        self.0.keep_alive_interval()
    }
    fn set_keep_alive_interval(&self, value: Duration) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.set-keep-alive-interval-time");
        self.0.set_keep_alive_interval(value)
    }
    fn keep_alive_count(&self) -> Result<u32, NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.keep-alive-count-time");
        self.0.keep_alive_count()
    }
    fn set_keep_alive_count(&self, value: u32) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.set-keep-alive-count-time");
        self.0.set_keep_alive_count(value)
    }
    fn hop_limit(&self) -> Result<u8, NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.hop-limit");
        self.0.hop_limit()
    }
    fn set_hop_limit(&self, value: u8) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.set-hop-limit");
        self.0.set_hop_limit(value)
    }
    fn receive_buffer_size(&self) -> Result<u64, NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.receive-buffer-size");
        self.0.receive_buffer_size()
    }
    fn set_receive_buffer_size(&self, value: u64) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.set-receive-buffer-size");
        self.0.set_receive_buffer_size(value)
    }
    fn send_buffer_size(&self) -> Result<u64, NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.send-buffer-size");
        self.0.send_buffer_size()
    }
    fn set_send_buffer_size(&self, value: u64) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.set-send-buffer-size");
        self.0.set_send_buffer_size(value)
    }
    fn subscribe(&self) -> Resource<Pollable> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.subscribe");
        Resource::new(Pollable::Host(self.0.subscribe()))
    }
    fn shutdown(&self, shutdown_type: ShutdownType) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/tcp#tcp-socket.shutdown");
        self.0.shutdown(match shutdown_type {
            ShutdownType::Receive => tcp::ShutdownType::Receive,
            ShutdownType::Send => tcp::ShutdownType::Send,
            ShutdownType::Both => tcp::ShutdownType::Both,
        })
    }
}

impl GuestUdpSocket for UdpSocket {
    fn start_bind(
        &self,
        network: &Network,
        local_address: IpSocketAddress,
    ) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#udp-socket.start-bind");
        self.0.start_bind(network, local_address)
    }
    fn finish_bind(&self) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#udp-socket.finish-bind");
        self.0.finish_bind()
    }
    fn local_address(&self) -> Result<IpSocketAddress, NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#udp-socket.local-address");
        self.0.local_address()
    }
    fn remote_address(&self) -> Result<IpSocketAddress, NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#udp-socket.remote-address");
        self.0.remote_address()
    }
    fn address_family(&self) -> IpAddressFamily {
        debug!("CALL wasi:sockets/udp#udp-socket.address-family");
        self.0.address_family()
    }
    fn unicast_hop_limit(&self) -> Result<u8, NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#udp-socket.unicast-hop-limit");
        self.0.unicast_hop_limit()
    }
    fn set_unicast_hop_limit(&self, value: u8) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#udp-socket.set-unicast-hop-limit");
        self.0.set_unicast_hop_limit(value)
    }
    fn receive_buffer_size(&self) -> Result<u64, NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#udp-socket.receive-buffer-size");
        self.0.receive_buffer_size()
    }
    fn set_receive_buffer_size(&self, value: u64) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#udp-socket.set-receive-buffer-size");
        self.0.set_receive_buffer_size(value)
    }
    fn send_buffer_size(&self) -> Result<u64, NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#udp-socket.send-buffer-size");
        self.0.send_buffer_size()
    }
    fn set_send_buffer_size(&self, value: u64) -> Result<(), NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#udp-socket.set-send-buffer-size");
        self.0.set_send_buffer_size(value)
    }
    fn subscribe(&self) -> Resource<Pollable> {
        debug!("CALL wasi:sockets/udp#udp-socket.subscribe");
        Resource::new(Pollable::Host(self.0.subscribe()))
    }
    fn stream(
        &self,
        remote_addr: Option<IpSocketAddress>,
    ) -> Result<
        (
            Resource<SocketsIncomingDatagramStream>,
            Resource<SocketsOutgoingDatagramStream>,
        ),
        NetworkErrorCode,
    > {
        debug!("CALL wasi:sockets/udp#udp-socket.stream");
        let (in_, out) = self.0.stream(remote_addr)?;
        Ok((
            Resource::new(SocketsIncomingDatagramStream(in_)),
            Resource::new(SocketsOutgoingDatagramStream(out)),
        ))
    }
}

impl GuestIncomingDatagramStream for SocketsIncomingDatagramStream {
    fn receive(&self, max_results: u64) -> Result<Vec<IncomingDatagram>, NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#incoming-datagram-stream.receive");
        match self.0.receive(max_results) {
            Ok(mut datagrams) => Ok(datagrams
                .drain(..)
                .map(|d| IncomingDatagram {
                    data: d.data,
                    remote_address: d.remote_address,
                })
                .collect::<Vec<IncomingDatagram>>()),
            Err(err) => Err(err),
        }
    }

    fn subscribe(&self) -> Resource<IoPollable> {
        debug!("CALL wasi:sockets/udp#incoming-datagram-stream.subscribe");
        Resource::new(Pollable::Host(self.0.subscribe()))
    }
}

impl GuestOutgoingDatagramStream for SocketsOutgoingDatagramStream {
    fn check_send(&self) -> Result<u64, NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#outgoing-datagram-stream.check-send");
        self.0.check_send()
    }

    fn send(&self, mut datagrams: Vec<OutgoingDatagram>) -> Result<u64, NetworkErrorCode> {
        debug!("CALL wasi:sockets/udp#outgoing-datagram-stream.send");
        self.0.send(
            datagrams
                .drain(..)
                .map(|d| udp::OutgoingDatagram {
                    data: d.data,
                    remote_address: d.remote_address,
                })
                .collect::<Vec<udp::OutgoingDatagram>>()
                .as_slice(),
        )
    }

    fn subscribe(&self) -> Resource<IoPollable> {
        debug!("CALL wasi:sockets/udp#outgoing-datagram-stream.subscribe");
        Resource::new(Pollable::Host(self.0.subscribe()))
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

fn stream_err_map(e: streams::StreamError) -> StreamError {
    match e {
        streams::StreamError::Closed => StreamError::Closed,
        streams::StreamError::LastOperationFailed(e) => {
            StreamError::LastOperationFailed(Resource::new(StreamsError::Host(e)))
        }
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

fn metadata_hash_map(value: filesystem_types::MetadataHashValue) -> MetadataHashValue {
    MetadataHashValue {
        upper: value.upper,
        lower: value.lower,
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

fn header_err_map_rev(err: http_types::HeaderError) -> HeaderError {
    match err {
        http_types::HeaderError::InvalidSyntax => HeaderError::InvalidSyntax,
        http_types::HeaderError::Forbidden => HeaderError::Forbidden,
        http_types::HeaderError::Immutable => HeaderError::Immutable,
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

fn http_err_map(err: HttpErrorCode) -> http_types::ErrorCode {
    match err {
        HttpErrorCode::DnsTimeout => http_types::ErrorCode::DnsTimeout,
        HttpErrorCode::DnsError(DnsErrorPayload { rcode, info_code }) => {
            http_types::ErrorCode::DnsError(http_types::DnsErrorPayload { rcode, info_code })
        }
        HttpErrorCode::DestinationNotFound => http_types::ErrorCode::DestinationNotFound,
        HttpErrorCode::DestinationUnavailable => http_types::ErrorCode::DestinationUnavailable,
        HttpErrorCode::DestinationIpProhibited => http_types::ErrorCode::DestinationIpProhibited,
        HttpErrorCode::DestinationIpUnroutable => http_types::ErrorCode::DestinationIpUnroutable,
        HttpErrorCode::ConnectionRefused => http_types::ErrorCode::ConnectionRefused,
        HttpErrorCode::ConnectionTerminated => http_types::ErrorCode::ConnectionTerminated,
        HttpErrorCode::ConnectionTimeout => http_types::ErrorCode::ConnectionTimeout,
        HttpErrorCode::ConnectionReadTimeout => http_types::ErrorCode::ConnectionReadTimeout,
        HttpErrorCode::ConnectionWriteTimeout => http_types::ErrorCode::ConnectionWriteTimeout,
        HttpErrorCode::ConnectionLimitReached => http_types::ErrorCode::ConnectionLimitReached,
        HttpErrorCode::TlsProtocolError => http_types::ErrorCode::TlsProtocolError,
        HttpErrorCode::TlsCertificateError => http_types::ErrorCode::TlsCertificateError,
        HttpErrorCode::TlsAlertReceived(TlsAlertReceivedPayload {
            alert_id,
            alert_message,
        }) => http_types::ErrorCode::TlsAlertReceived(http_types::TlsAlertReceivedPayload {
            alert_id,
            alert_message,
        }),
        HttpErrorCode::HttpRequestDenied => http_types::ErrorCode::HttpRequestDenied,
        HttpErrorCode::HttpRequestLengthRequired => {
            http_types::ErrorCode::HttpRequestLengthRequired
        }
        HttpErrorCode::HttpRequestBodySize(s) => http_types::ErrorCode::HttpRequestBodySize(s),
        HttpErrorCode::HttpRequestMethodInvalid => http_types::ErrorCode::HttpRequestMethodInvalid,
        HttpErrorCode::HttpRequestUriInvalid => http_types::ErrorCode::HttpRequestUriInvalid,
        HttpErrorCode::HttpRequestUriTooLong => http_types::ErrorCode::HttpRequestUriTooLong,
        HttpErrorCode::HttpRequestHeaderSectionSize(s) => {
            http_types::ErrorCode::HttpRequestHeaderSectionSize(s)
        }
        HttpErrorCode::HttpRequestHeaderSize(Some(FieldSizePayload {
            field_name,
            field_size,
        })) => http_types::ErrorCode::HttpRequestHeaderSize(Some(http_types::FieldSizePayload {
            field_name,
            field_size,
        })),
        HttpErrorCode::HttpRequestHeaderSize(None) => {
            http_types::ErrorCode::HttpRequestHeaderSize(None)
        }
        HttpErrorCode::HttpRequestTrailerSectionSize(s) => {
            http_types::ErrorCode::HttpRequestTrailerSectionSize(s)
        }
        HttpErrorCode::HttpRequestTrailerSize(FieldSizePayload {
            field_name,
            field_size,
        }) => http_types::ErrorCode::HttpRequestTrailerSize(http_types::FieldSizePayload {
            field_name,
            field_size,
        }),
        HttpErrorCode::HttpResponseIncomplete => http_types::ErrorCode::HttpResponseIncomplete,
        HttpErrorCode::HttpResponseHeaderSectionSize(s) => {
            http_types::ErrorCode::HttpResponseHeaderSectionSize(s)
        }
        HttpErrorCode::HttpResponseHeaderSize(FieldSizePayload {
            field_name,
            field_size,
        }) => http_types::ErrorCode::HttpResponseHeaderSize(http_types::FieldSizePayload {
            field_name,
            field_size,
        }),
        HttpErrorCode::HttpResponseBodySize(s) => http_types::ErrorCode::HttpResponseBodySize(s),
        HttpErrorCode::HttpResponseTrailerSectionSize(s) => {
            http_types::ErrorCode::HttpResponseTrailerSectionSize(s)
        }
        HttpErrorCode::HttpResponseTrailerSize(FieldSizePayload {
            field_name,
            field_size,
        }) => http_types::ErrorCode::HttpResponseTrailerSize(http_types::FieldSizePayload {
            field_name,
            field_size,
        }),
        HttpErrorCode::HttpResponseTransferCoding(e) => {
            http_types::ErrorCode::HttpResponseTransferCoding(e)
        }
        HttpErrorCode::HttpResponseContentCoding(e) => {
            http_types::ErrorCode::HttpResponseContentCoding(e)
        }
        HttpErrorCode::HttpResponseTimeout => http_types::ErrorCode::HttpResponseTimeout,
        HttpErrorCode::HttpUpgradeFailed => http_types::ErrorCode::HttpUpgradeFailed,
        HttpErrorCode::HttpProtocolError => http_types::ErrorCode::HttpProtocolError,
        HttpErrorCode::LoopDetected => http_types::ErrorCode::LoopDetected,
        HttpErrorCode::ConfigurationError => http_types::ErrorCode::ConfigurationError,
        HttpErrorCode::InternalError(e) => http_types::ErrorCode::InternalError(e),
    }
}

fn http_err_map_rev(err: http_types::ErrorCode) -> HttpErrorCode {
    match err {
        http_types::ErrorCode::DnsTimeout => HttpErrorCode::DnsTimeout,
        http_types::ErrorCode::DnsError(http_types::DnsErrorPayload { rcode, info_code }) => {
            HttpErrorCode::DnsError(DnsErrorPayload { rcode, info_code })
        }
        http_types::ErrorCode::DestinationNotFound => HttpErrorCode::DestinationNotFound,
        http_types::ErrorCode::DestinationUnavailable => HttpErrorCode::DestinationUnavailable,
        http_types::ErrorCode::DestinationIpProhibited => HttpErrorCode::DestinationIpProhibited,
        http_types::ErrorCode::DestinationIpUnroutable => HttpErrorCode::DestinationIpUnroutable,
        http_types::ErrorCode::ConnectionRefused => HttpErrorCode::ConnectionRefused,
        http_types::ErrorCode::ConnectionTerminated => HttpErrorCode::ConnectionTerminated,
        http_types::ErrorCode::ConnectionTimeout => HttpErrorCode::ConnectionTimeout,
        http_types::ErrorCode::ConnectionReadTimeout => HttpErrorCode::ConnectionReadTimeout,
        http_types::ErrorCode::ConnectionWriteTimeout => HttpErrorCode::ConnectionWriteTimeout,
        http_types::ErrorCode::ConnectionLimitReached => HttpErrorCode::ConnectionLimitReached,
        http_types::ErrorCode::TlsProtocolError => HttpErrorCode::TlsProtocolError,
        http_types::ErrorCode::TlsCertificateError => HttpErrorCode::TlsCertificateError,
        http_types::ErrorCode::TlsAlertReceived(http_types::TlsAlertReceivedPayload {
            alert_id,
            alert_message,
        }) => HttpErrorCode::TlsAlertReceived(TlsAlertReceivedPayload {
            alert_id,
            alert_message,
        }),
        http_types::ErrorCode::HttpRequestDenied => HttpErrorCode::HttpRequestDenied,
        http_types::ErrorCode::HttpRequestLengthRequired => {
            HttpErrorCode::HttpRequestLengthRequired
        }
        http_types::ErrorCode::HttpRequestBodySize(s) => HttpErrorCode::HttpRequestBodySize(s),
        http_types::ErrorCode::HttpRequestMethodInvalid => HttpErrorCode::HttpRequestMethodInvalid,
        http_types::ErrorCode::HttpRequestUriInvalid => HttpErrorCode::HttpRequestUriInvalid,
        http_types::ErrorCode::HttpRequestUriTooLong => HttpErrorCode::HttpRequestUriTooLong,
        http_types::ErrorCode::HttpRequestHeaderSectionSize(s) => {
            HttpErrorCode::HttpRequestHeaderSectionSize(s)
        }
        http_types::ErrorCode::HttpRequestHeaderSize(Some(http_types::FieldSizePayload {
            field_name,
            field_size,
        })) => HttpErrorCode::HttpRequestHeaderSize(Some(FieldSizePayload {
            field_name,
            field_size,
        })),
        http_types::ErrorCode::HttpRequestHeaderSize(None) => {
            HttpErrorCode::HttpRequestHeaderSize(None)
        }
        http_types::ErrorCode::HttpRequestTrailerSectionSize(s) => {
            HttpErrorCode::HttpRequestTrailerSectionSize(s)
        }
        http_types::ErrorCode::HttpRequestTrailerSize(http_types::FieldSizePayload {
            field_name,
            field_size,
        }) => HttpErrorCode::HttpRequestTrailerSize(FieldSizePayload {
            field_name,
            field_size,
        }),
        http_types::ErrorCode::HttpResponseIncomplete => HttpErrorCode::HttpResponseIncomplete,
        http_types::ErrorCode::HttpResponseHeaderSectionSize(s) => {
            HttpErrorCode::HttpResponseHeaderSectionSize(s)
        }
        http_types::ErrorCode::HttpResponseHeaderSize(http_types::FieldSizePayload {
            field_name,
            field_size,
        }) => HttpErrorCode::HttpResponseHeaderSize(FieldSizePayload {
            field_name,
            field_size,
        }),
        http_types::ErrorCode::HttpResponseBodySize(s) => HttpErrorCode::HttpResponseBodySize(s),
        http_types::ErrorCode::HttpResponseTrailerSectionSize(s) => {
            HttpErrorCode::HttpResponseTrailerSectionSize(s)
        }
        http_types::ErrorCode::HttpResponseTrailerSize(http_types::FieldSizePayload {
            field_name,
            field_size,
        }) => HttpErrorCode::HttpResponseTrailerSize(FieldSizePayload {
            field_name,
            field_size,
        }),
        http_types::ErrorCode::HttpResponseTransferCoding(e) => {
            HttpErrorCode::HttpResponseTransferCoding(e)
        }
        http_types::ErrorCode::HttpResponseContentCoding(e) => {
            HttpErrorCode::HttpResponseContentCoding(e)
        }
        http_types::ErrorCode::HttpResponseTimeout => HttpErrorCode::HttpResponseTimeout,
        http_types::ErrorCode::HttpUpgradeFailed => HttpErrorCode::HttpUpgradeFailed,
        http_types::ErrorCode::HttpProtocolError => HttpErrorCode::HttpProtocolError,
        http_types::ErrorCode::LoopDetected => HttpErrorCode::LoopDetected,
        http_types::ErrorCode::ConfigurationError => HttpErrorCode::ConfigurationError,
        http_types::ErrorCode::InternalError(e) => HttpErrorCode::InternalError(e),
    }
}

// This function gets mutated by the virtualizer
#[no_mangle]
#[inline(never)]
pub fn passive_alloc(passive_idx: u32, offset: u32, len: u32) -> *mut u8 {
    return (passive_idx + offset + len) as *mut u8;
}
