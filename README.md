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

### Explainer

When wanting to run WebAssembly Components depending on WASI APIs in other environments it can provide
a point of friction having to port WASI interop to every target platform.

In addition having full unrestricted access to core operating system APIs is a security concern.

WASI Virt allows taking a component that depends on WASI APIs and using a virtualized adapter to convert
it into a component that no longer depends on those WASI APIs, or conditionally only depends on them in 
a configurable way.

For example, consider converting an application to a WebAssembly Component that assumes it can load
some templates from the filesystem, but that is all it will load.

Using WASI Virt, those specific file paths can be mounted and virtualized into the component itself as 
a post-compile operation, while banning the final component from being able to access the filesystem at
all. The inner program still uses FS calls, but they are virtualized from the target host platform allowing
this application to run in different environments without filesystem API compat or security concerns.

### Basic Usage

```rs
use std::fs;
use wasi_virt::{WasiVirt, FsEntry};

fn main() {
    let virt_component_bytes = WasiVirt::new()
        // provide an allow list of host env vars
        .env_host_allow(&["PUBLIC_ENV_VAR"])
        // provide custom env overrides
        .env_overrides(&[("SOME", "ENV"), ("VAR", "OVERRIDES")])
        // mount and virtualize a local directory recursively
        .fs_preopen("/dir", FsEntry::Virtualize("/local/dir"))
        // create a virtual directory containing some virtual files
        .fs_preopen("/another-dir", FsEntry::Dir(BTreeMap::from([
          // create a virtual file from the given UTF8 source
          ("file.txt", FsEntry::Source("Hello world")),
          // create a virtual file read from a local file at
          // virtualization time
          ("another.wasm", FsEntry::Virtualize("/local/another.wasm"))
          // create a virtual file which reads from a given file
          // path at runtime using the runtime host filesystem API
          ("host.txt", FsEntry::RuntimeFile("/runtime/host/path.txt"))
        ])))
        .create()
        .unwrap();
    fs::write("virt.component.wasm", virt_component_bytes).unwrap();
}
```

With the created `virt.component.wasm` component, this can now be composed into a component with the `wasm-tools compose` "definitions" feature:

```
wasm-tools compose mycomponent.wasm -d virt.component.wasm -o out.component.wasm
```

When configuring a virtualization that does not fall back to the host, imports to the subsystem will be entirely stripped from the component.

## CLI

A CLI is also provided in this crate supporting:

```
wasi-virt config.toml -o virt.wasm
```

### Configuration

With the configuration file format:

```
### Environment Virtualization
[env]
### Set environment variable values:
overrides = [["CUSTOM", "VAL"]]
### Enable environment vars for the host:
host = "all"
### Alternatively create an allow list:
# [env.host]
# allow = ["ENV_KEY"]
### or deny list:
# [env.host]
# deny = ["ENV_KEY"]

### FS Virtualization

### Create a virtual directory with file.txt from
### the provided inline UTF8 string, and with another.wasm
### inlined into the virtual adapter from the local filesystem
### path at virtualization time:
[fs.preopens."/".dir]
"file.txt" = { source = "inner contents" }
"another.wasm" = { virtualize = "/local/path/to/another.wasm" }

### Mount a local directory as a virtualized directory:
[fs.preopens."/dir"]
virtualize = "/local/path"

### Mount a passthrough runtime host directory:
[fs.preopens."/runtime-host"]
runtime = "/runtime/path"
```

# License

This project is licensed under the Apache 2.0 license with the LLVM exception.
See [LICENSE](LICENSE) for more details.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you, as defined in the Apache-2.0 license,
shall be licensed as above, without any additional terms or conditions.
