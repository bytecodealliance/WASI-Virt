#![no_main]
#![feature(ptr_sub_ptr)]

mod env;
mod io;

pub(crate) struct VirtAdapter;

wit_bindgen::generate!({
    path: "../wit",
    world: "virtual-adapter",
    exports: {
        "wasi:poll/poll": VirtAdapter,
        "wasi:io/streams": VirtAdapter,
        "wasi:filesystem/filesystem": VirtAdapter,
        "wasi:filesystem/preopens": VirtAdapter,
        "wasi:filesystem/types": VirtAdapter,
        "wasi:cli/environment": VirtAdapter,
        "wasi:cli/stdin": VirtAdapter,
        "wasi:cli/stdout": VirtAdapter,
        "wasi:cli/stderr": VirtAdapter,
        "wasi:cli/stderr": VirtAdapter,
        "wasi:cli/terminal-input": VirtAdapter,
        "wasi:cli/terminal-output": VirtAdapter,
        "wasi:cli/terminal-stdin": VirtAdapter,
        "wasi:cli/terminal-stdout": VirtAdapter,
        "wasi:cli/terminal-stderr": VirtAdapter,
        "wasi:poll/poll": VirtAdapter,
        "wasi:clocks/monotonic-clock": VirtAdapter,
        "wasi:http/types": VirtAdapter,
        "wasi:sockets/ip-name-lookup": VirtAdapter,
        "wasi:sockets/tcp": VirtAdapter,
        "wasi:sockets/udp": VirtAdapter,
    }
});
