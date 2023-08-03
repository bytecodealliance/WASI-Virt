#![no_main]

mod env;
mod io;

pub(crate) struct VirtAdapter;

wit_bindgen::generate!({
    path: "../wit",
    world: "virtual-adapter",
    exports: {
        "wasi:poll/poll": VirtAdapter,
        "wasi:io/streams": VirtAdapter,
        "wasi:cli-base/environment": VirtAdapter,
        "wasi:filesystem/filesystem": VirtAdapter,
        "wasi:cli-base/preopens": VirtAdapter,
        "wasi:cli-base/stdin": VirtAdapter,
        "wasi:cli-base/stdout": VirtAdapter,
        "wasi:cli-base/stderr": VirtAdapter,
        "wasi:cli-base/stderr": VirtAdapter,
        "wasi:poll/poll": VirtAdapter,
        "wasi:clocks/monotonic-clock": VirtAdapter,
        "wasi:http/types": VirtAdapter,
        "wasi:sockets/ip-name-lookup": VirtAdapter,
        "wasi:sockets/tcp": VirtAdapter,
        "wasi:sockets/udp": VirtAdapter,
    }
});
