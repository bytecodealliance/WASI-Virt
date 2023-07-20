#![no_main]
#![feature(ptr_sub_ptr)]

mod env;
mod fs;

wit_bindgen::generate!({
    path: "../wit",
    world: "virtual-adapter"
});

pub(crate) struct VirtAdapter;

export_virtual_adapter!(VirtAdapter);
