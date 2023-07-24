<div align="center">
  <h1><code>WASI Virt</code></h1>

  <p>
    <strong>Virtualization Component Generator for WASI Preview 2</strong>
  </p>

  <strong>A <a href="https://bytecodealliance.org/">Bytecode Alliance</a> project</strong>

  <p>
    <a href="https://github.com/bytecodealliance/wasi-virt/actions?query=workflow%3ACI"><img src="https://github.com/bytecodealliance/wasi-virt/workflows/CI/badge.svg" alt="build status" /></a>
  </p>
</div>

The virtualized component can be composed into a WASI Preview2 component with `wasm-tools compose`, providing fully-configurable WASI virtualization with host pass through or full encapsulation as needed.

Subsystem support:

- [x] Environment virtualization
- [x] Filesystem virtualization
- [ ] Stdio
- [ ] Sockets
- [ ] Clocks
- [ ] [Your suggestion here](https://github.com/bytecodealliance/WASI-Virt/issues/new)

While current virtualization support is limited, the goal for this project is to support a wide range of WASI virtualization use cases.

## Explainer

When wanting to run WebAssembly Components depending on WASI APIs in other environments it can provide
a point of friction having to port WASI interop to every target platform.

In addition having full unrestricted access to core operating system APIs is a security concern.

WASI Virt allows taking a component that depends on WASI APIs and using a virtualized adapter to convert
it into a component that no longer depends on those WASI APIs, or conditionally only depends on them in 
a configurable way.

For example, consider converting an application to a WebAssembly Component that assumes it can load
read some files from the filesystem, but never needs to write.

Using WASI Virt, those specific file paths can be mounted and virtualized into the component itself as 
a post-compile operation, while banning the final component from being able to access the host's filesystem at
all. The inner program still imports a wasi filesystem, but the filesystem implementation is provided by another component, rather than in the host environment. The composition of these two components no longer has a
filesystem import, so it can be run in hosts (or other components) which do not provide a filesystem API.

## Basic Usage

```
cargo install wasi-virt
```

### Env Virtualization

```
# Encapsulating a component
wasi-virt component.wasm -o virt.wasm

# Setting specific env vars (while disallowing all host env var access):
wasi-virt component.wasm -e CUSTOM=VAR -o virt.wasm

# Setting env vars with all host env vars allowed:
wasi-virt component.wasm -e CUSTOM=VAR --allow-env -o virt.wasm

# Setting env vars with restricted host env var access:
wasi-virt component.wasm -e CUSTOM=VAR --allow-env=SOME,ENV_VARS -o virt.wasm
```

### FS Virtualization

```
# Mounting a virtual directory
# (Globs all files in local-dir and virtualizes them)
wasi-virt component.wasm --mount /=./local-dir -o virt.wasm

# Providing a host preopen mapping
wasi-virt component.wasm --preopen /=/restricted/path -o virt.wasm

# Providing both host and virtual preopens
wasi-virt component.wasm --mount /virt-dir=./local --preopen /host-dir=/host/path -o virt.wasm
```

By default, all virtualizations encapsulate the host virtualization, unless explicitly enabling
host passthrough via `--allow-env` or `--preopen`.

In all of the above examples, the `component.wasm` argument is optional. If omitted, then the virtualized
adapter is output into `virt.wasm`, which can be composed into any component with:

```
wasm-tools compose component.wasm -d virt.wasm -o component.virt.wasm
```

## API

```rs
use std::fs;
use wasi_virt::{WasiVirt, FsEntry};

fn main() {
    let virt = WasiVirt::new();
    virt.env()
      // provide an allow list of host env vars
      .allow(&["PUBLIC_ENV_VAR"])
      // provide custom env overrides
      .overrides(&[("SOME", "ENV"), ("VAR", "OVERRIDES")]);
        
    virt.fs()
        // mount and virtualize a local directory recursively
        .virtual_preopen("/dir", "/local/dir")
        // create a virtual directory containing some virtual files
        .preopen("/another-dir", FsEntry::Dir(BTreeMap::from([
          // create a virtual file from the given UTF8 source
          ("file.txt", FsEntry::Source("Hello world")),
          // create a virtual file read from a local file at
          // virtualization time
          ("another.wasm", FsEntry::Virtualize("/local/another.wasm"))
          // create a virtual file which reads from a given file
          // path at runtime using the runtime host filesystem API
          ("host.txt", FsEntry::RuntimeFile("/runtime/host/path.txt"))
        ])));

    let virt_component_bytes = virt.finish().unwrap();
    fs::write("virt.component.wasm", virt_component_bytes).unwrap();
}
```

When calling a subsystem for the first time, its virtualization will be enabled. Subsystems
not used or configured at all will be omitted from the virtualization entirely.

# License

This project is licensed under the Apache 2.0 license with the LLVM exception.
See [LICENSE](LICENSE) for more details.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you, as defined in the Apache-2.0 license,
shall be licensed as above, without any additional terms or conditions.
