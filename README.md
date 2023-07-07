<div align="center">
  <h1><code>WASI Virt</code></h1>

  <p>
    <strong>Virtualization Component Generator for WASI Preview 2</strong>
  </p>

  <strong>A <a href="https://bytecodealliance.org/">Bytecode Alliance</a> project</strong>

  <p>
    <a href="https://github.com/bytecodealliance/jco/actions?query=workflow%3ACI"><img src="https://github.com/bytecodealliance/jco/workflows/CI/badge.svg" alt="build status" /></a>
  </p>
</div>

The virtualized component can be composed into a WASI Preview2 component with `wasm-tools compose`, providing fully-configurable WASI virtualization with host pass through or full encapsulation as needed.

Subsystem support:

- [x] Environment virtualization
- [ ] Filesystem virtualization

### Example

```rs
use std::fs;
use wasi_virt::WasiVirt;

fn main() {
    let virt_component_bytes = WasiVirt::new()
        // provide an allow list of host env vars
        .env_host_allow(&["PUBLIC_ENV_VAR"])
        // provide custom env overrides
        .env_overrides(&[("SOME", "ENV"), ("VAR", "OVERRIDES")])
        .create()
        .unwrap();
    fs::write("virt.component.wasm", virt_component_bytes).unwrap();
}
```

With the created `virt.component.wasm` component, this can now be composed into a component with the `wasm-tools compose` "definitions" feature:

```
wasm-tools compose mycomponent.wasm -d virt.component.wasm -o out.component.wasm
```

# License

This project is licensed under the Apache 2.0 license with the LLVM exception.
See [LICENSE](LICENSE) for more details.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you, as defined in the Apache-2.0 license,
shall be licensed as above, without any additional terms or conditions.
