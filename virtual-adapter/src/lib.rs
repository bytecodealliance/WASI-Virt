#![no_main]
#![feature(ptr_sub_ptr)]

mod env;
mod io;

pub(crate) struct VirtAdapter;

wit_bindgen::generate!({
    path: "../wit",
    world: "virtual-adapter",
    exports: {
        "wasi:io/poll": VirtAdapter,
        "wasi:io/poll/pollable": io::IoPollable,
        "wasi:io/error/error": io::IoError,
        "wasi:io/streams/input-stream": io::IoInputStream,
        "wasi:io/streams/output-stream": io::IoOutputStream,
        "wasi:filesystem/preopens": VirtAdapter,
        "wasi:filesystem/types": VirtAdapter,
        "wasi:filesystem/types/descriptor": io::FilesystemDescriptor,
        "wasi:filesystem/types/directory-entry-stream": io::FilesystemDirectoryEntryStream,
        "wasi:cli/environment": VirtAdapter,
        "wasi:cli/stdin": VirtAdapter,
        "wasi:cli/stdout": VirtAdapter,
        "wasi:cli/stderr": VirtAdapter,
        "wasi:cli/terminal-input/terminal-input": io::CliTerminalInput,
        "wasi:cli/terminal-output/terminal-output": io::CliTerminalOutput,
        "wasi:cli/terminal-stdin": VirtAdapter,
        "wasi:cli/terminal-stdout": VirtAdapter,
        "wasi:cli/terminal-stderr": VirtAdapter,
        "wasi:clocks/monotonic-clock": VirtAdapter,
        "wasi:http/types/fields": io::HttpFields,
        "wasi:http/types/future-incoming-response": io::HttpFutureIncomingResponse,
        "wasi:http/types/future-trailers": io::HttpFutureTrailers,
        "wasi:http/types/incoming-body": io::HttpIncomingBody,
        "wasi:http/types/incoming-request": io::HttpIncomingRequest,
        "wasi:http/types/incoming-response": io::HttpIncomingResponse,
        "wasi:http/types/outgoing-body": io::HttpOutgoingBody,
        "wasi:http/types/outgoing-request": io::HttpOutgoingRequest,
        "wasi:http/types/outgoing-response": io::HttpOutgoingResponse,
        "wasi:http/types/response-outparam": io::HttpResponseOutparam,
        "wasi:http/outgoing-handler": VirtAdapter,
        "wasi:sockets/ip-name-lookup": VirtAdapter,
        "wasi:sockets/ip-name-lookup/resolve-address-stream": io::SocketsResolveAddressStream,
        "wasi:sockets/tcp/tcp-socket": io::SocketsTcpSocket,
        "wasi:sockets/udp/udp-socket": io::SocketsUdpSocket,
        "wasi:sockets/udp/incoming-datagram-stream": io::SocketsIncomingDatagramStream,
        "wasi:sockets/udp/outgoing-datagram-stream": io::SocketsOutgoingDatagramStream,
    }
});
