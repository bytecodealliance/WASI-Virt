#![no_main]

mod env;
mod io;

wit_bindgen::generate!({
    path: "../wit",
    world: "virtual-adapter"
});

pub(crate) struct VirtAdapter;

export_virtual_adapter!(VirtAdapter);
