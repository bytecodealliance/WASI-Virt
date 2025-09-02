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

Supports all of the current WASI subsystems:

- [Clocks](#clocks): Allow / Deny
- [Environment](#env): Set environment variables, configure host environment variable permissions
- [Config](#config): Set configuration, configure host property permissions
- [Exit](#exit): Allow / Deny
- [Filesystem](#filesystem): Mount a read-only filesystem, configure host filesystem preopen remappings or pass-through.
- [HTTP](#http): Allow / Deny
- [Random](#random): Allow / Deny
- [Sockets](#sockets): Allow / Deny
- [Stdio](#stdio): Allow / Deny / Ignore

While current virtualization support is limited, the goal for this project is to support a wide range of WASI virtualization configuration use cases.

Have an unhandled use case? Post a virtualization [suggestion](https://github.com/bytecodealliance/WASI-Virt/issues/new).

## Explainer

When wanting to run WebAssembly Components depending on WASI APIs in other environments it can provide a point of friction having to port WASI interop to every target platform.

In addition having full unrestricted access to core operating system APIs is a security concern.

WASI Virt allows taking a component that depends on WASI APIs and using a virtualized adapter to convert it into a component that no longer depends on those WASI APIs, or conditionally only depends on them in a configurable way.

For example, consider converting an application to a WebAssembly Component that assumes it can load read some files from the filesystem, but never needs to write.

Using WASI Virt, those specific file paths can be mounted and virtualized into the component itself as a post-compile operation, while banning the final component from being able to access the host's filesystem at all. The inner program still imports a wasi filesystem, but the filesystem implementation is provided by another component, rather than in the host environment. The composition of these two components no longer has a filesystem import, so it can be run in hosts (or other components) which do not provide a filesystem API.

## Basic Usage

```sh
cargo install --git https://github.com/bytecodealliance/wasi-virt
```

```sh
# Encapsulating a component
wasi-virt component.wasm -o virt.wasm
```

By default the virtualization will deny all subsystems, and will panic on any attempt to use any subsystem.

In all of the examples, the `component.wasm` is the component being virtualized and `virt.wasm` the output component with its virtualization. The `component.wasm` argument is optional. If omitted, then the virtualized adapter is output into `virt.wasm`, which can be composed into any arbitrary component with:

```sh
wasm-tools compose component.wasm -d virt.wasm -o component.virt.wasm
```

Configuring a subsystem virtualization will enable it, or subsystems can be fully enabled via `--allow-fs`, `--allow-env` etc by subsystem. Allowing all subsystems can be achieved with `--allow-all`.

### Clocks

```sh
# Create a component which just allows clocks, but no other interfaces
wasi-virt component.wasm --allow-clocks -o virt.wasm
```

### Env

```sh
# Setting specific env vars (while disallowing all host env var access):
wasi-virt component.wasm -e CUSTOM=VAR -o virt.wasm

# Setting env vars with all host env vars allowed:
wasi-virt component.wasm -e CUSTOM=VAR --allow-env -o virt.wasm

# Setting env vars with restricted host env var access:
wasi-virt component.wasm -e CUSTOM=VAR --allow-env=SOME,ENV_VARS -o virt.wasm
```

### Config

_experimental_

```sh
# Setting specific config properties (while disallowing all host config property access):
wasi-virt component.wasm -c custom=prop -o virt.wasm

# Setting config properties with all host config properties allowed:
wasi-virt component.wasm -c custom=prop --allow-config -o virt.wasm

# Setting config properties with restricted host config property access:
wasi-virt component.wasm -c custom=prop --allow-config=some,property -o virt.wasm
```

### Exit

```sh
# Create a component which is allowed to exit (terminate execution without a panic and unwind)
wasi-virt component.wasm --allow-exit -o virt.wasm
```

### Filesystem

```sh
# Mounting a virtual directory
# (Globs all files in local-dir and virtualizes them)
wasi-virt component.wasm --mount /=./local-dir -o virt.wasm

# Providing a host preopen mapping
wasi-virt component.wasm --preopen /=/restricted/path -o virt.wasm

# Providing both host and virtual preopens
wasi-virt component.wasm --mount /virt-dir=./local --preopen /host-dir=/host/path -o virt.wasm
```

### HTTP

```sh
# Allow HTTP
wasi-virt component.wasm --allow-http -o virt.wasm
```

### Random

```sh
# Allow random number generation
wasi-virt component.wasm --allow-random -o virt.wasm
```

### Sockets

```sh
# Allow socket APIs
wasi-virt component.wasm --allow-sockets -o virt.wasm
```

### Stdio

```sh
# Ignore all stdio entirely
wasi-virt component.wasm --stdio=ignore -o virt.wasm

# Throw an error if attempting any stdio
# (this is the default)
wasi-virt component.wasm --stdio=deny -o virt.wasm

# Configure stderr / stdout / stdin individually
wasi-virt component.wasm --stderr=allow -o virt.wasm
```

### Debugging

The `--debug` flag can be used to get tracing output of system calls.

## API

When using the virtualization API, subsystems are passthrough by default instead of deny by default.

```rs
use std::fs;
use wasi_virt::{WasiVirt, FsEntry};

fn main() {
    let mut virt = WasiVirt::new();

    // Allow all subsystems initially
    virt.allow_all();

    // Ignore stdio
    virt.stdio().ignore();

    virt.env()
      // Provide an allow list of host env vars
      .allow(&["PUBLIC_ENV_VAR"])
      // Provide custom env overrides
      .overrides(&[("SOME", "ENV"), ("VAR", "OVERRIDES")]);

    virt.fs()
        // Deny arbitrary host preopens
        .deny_host_preopens()
        // Mount and virtualize a local directory recursively
        .virtual_preopen("/dir", "/local/dir")
        // Create a virtual directory containing some virtual files
        .preopen("/another-dir", FsEntry::Dir(BTreeMap::from([
          // Create a virtual file from the given UTF8 source
          ("file.txt", FsEntry::Source("Hello world")),
          // Create a virtual file read from a local file at virtualization time
          ("another.wasm", FsEntry::Virtualize("/local/another.wasm"))
          // Create a virtual file which reads from a given file
          // path at runtime using the runtime host filesystem API
          ("host.txt", FsEntry::RuntimeFile("/runtime/host/path.txt"))
        ])));

    // Compose the virtualization with a component
    virt.compose("path/to/component.wasm");

    // Filter enabled subsystems by imports on the composed component
    virt.filter_imports().unwrap();

    // Finalize the virtualization
    let virt_component_bytes = virt.finish().unwrap();

    // Write out the virtualized component
    fs::write("virt.component.wasm", virt_component_bytes).unwrap();
}
```

When calling a subsystem for the first time, its virtualization will be enabled. Subsystems not used or configured at all will be omitted from the virtualization entirely.

### Selective Subsystem Virtualization

By default, when using the `wasi-virt` CLI command, all virtualizations are enabled. This way, not only is encapsulation the default, but composition with arbitrary components will always work out as all interfaces for WASI should always be available.

Selective subsystem virtualization can be performed directly with the WASI Virt library as above (which does not virtualize all subsystems by default). This allows virtualizing just a single subsystem like `env`, where it is possible to virtualize only that subsystem and skip other virtualizations and end up creating a smaller virtualization component.

When directly composing another component, individual subsystems are disabled if the composed component does not import that subsystem. This behavior reduces imports in the output component that are never actually used.

There is an important caveat to this: _as soon as any subsystem uses IO, all subsystems using IO need to be virtualized in order to fully subclass streams and polls in the virtualization layer_. In future this caveat requirement should weaken as these features lower into the core ABI in subsequent WASI versions.

`wasm-tools compose` will error in these scenarios, and better [error messaging](https://github.com/bytecodealliance/wasm-tools/issues/1147) may be provided in future when performing invalid compositions like the above. Missing subsystems can be usually detected from the composition warnings list.

## Contributing

> [!NOTE]
> You may need to install the `wasm32-wasip1` target if not already added (`rustup target add wasm32-wasip1`).

To build, run `./build-adapter.sh` which builds the master `virtual-adapter` component, followed by `cargo build` to build
the virtualization tooling (located in `src`).

Tests can be run with `cargo test`, which runs the tests in `tests/virt.rs`.

Test components are built from the `tests/components` directory, and run against the configurations provided in `tests/cases`.

To update WASI, `lib/wasi_snapshot_preview1.reactor.wasm` needs
to be updated to the latest Wasmtime build, and the `wit/deps` folder needs to be updated with the latest WASI definitions.

# License

This project is licensed under the Apache 2.0 license with the LLVM exception.
See [LICENSE](LICENSE) for more details.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you, as defined in the Apache-2.0 license,
shall be licensed as above, without any additional terms or conditions.
