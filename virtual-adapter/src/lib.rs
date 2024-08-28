#![no_main]
#![feature(ptr_sub_ptr)]

mod config;
mod env;
mod io;

pub(crate) struct VirtAdapter;

wit_bindgen::generate!({
    path: "../wit",
    world: "virtual-adapter",
    generate_all
});

export!(VirtAdapter);
