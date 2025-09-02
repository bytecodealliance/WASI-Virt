#![no_main]

mod config;
mod env;
mod io;

pub(crate) struct VirtAdapter;

pub(crate) mod bindings {
    #[cfg(all(feature = "wasi-0_2_1", not(feature = "wasi-0_2_3")))]
    wit_bindgen::generate!({
        path: "../wit/0_2_1",
        world: "virtual-adapter",
        generate_all
    });

    #[cfg(all(feature = "wasi-0_2_3", not(feature = "wasi-0_2_1")))]
    wit_bindgen::generate!({
        path: "../wit/0_2_3",
        world: "virtual-adapter",
        generate_all
    });

    #[cfg(all(not(feature = "wasi-0_2_3"), not(feature = "wasi-0_2_1")))]
    compile_error!("a wasi feature must be provided");

    #[cfg(all(feature = "wasi-0_2_3", feature = "wasi-0_2_1"))]
    compile_error!("wasi features are mutually exclusive");

    use super::VirtAdapter;
    export!(VirtAdapter);
}
