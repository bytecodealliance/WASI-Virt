use std::fs;
use wasi_virt::WasiVirt;

fn main() {
    let virt_component_bytes = WasiVirt::new()
        .env_host_allow(&["PUBLIC_ENV_VAR"])
        .env_overrides(&[("SOME", "ENV"), ("VAR", "OVERRIDES")])
        .create()
        .unwrap();
    fs::write("virt.component.wasm", virt_component_bytes).unwrap();
}
