use anyhow::{bail, Context, Result};
use walrus::Module;

use super::StubRequirement;

/// Imports exposed by WASI for IO functionality
///
/// Some imports are required, and others are optional.
const WASI_IO_IMPORTS: &[(&str, &str, &StubRequirement)] = &[
    (
        "wasi:io/streams@0.2.0",
        "[method]output-stream.blocking-flush",
        &StubRequirement::Optional,
    ),
    (
        "wasi:io/streams@0.2.0",
        "[method]input-stream.blocking-read",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/streams@0.2.0",
        "[method]input-stream.blocking-skip",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/streams@0.2.0",
        "[method]output-stream.blocking-splice",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/streams@0.2.0",
        "[method]output-stream.blocking-write-and-flush",
        &StubRequirement::Optional,
    ),
    (
        "wasi:io/streams@0.2.0",
        "[method]output-stream.check-write",
        &StubRequirement::Optional,
    ),
    (
        "wasi:io/streams@0.2.0",
        "[resource-drop]input-stream",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/streams@0.2.0",
        "[resource-drop]output-stream",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/streams@0.2.0",
        "[method]output-stream.flush",
        &StubRequirement::Optional,
    ),
    (
        "wasi:io/streams@0.2.0",
        "[method]input-stream.read",
        &StubRequirement::Optional,
    ),
    (
        "wasi:io/streams@0.2.0",
        "[method]input-stream.skip",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/streams@0.2.0",
        "[method]output-stream.splice",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/streams@0.2.0",
        "[method]input-stream.subscribe",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/streams@0.2.0",
        "[method]output-stream.subscribe",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/streams@0.2.0",
        "[method]output-stream.write",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/streams@0.2.0",
        "[method]output-stream.write-zeroes",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/streams@0.2.0",
        "[method]output-stream.blocking-write-zeroes-and-flush",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/error@0.2.0",
        "[resource-drop]error",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/poll@0.2.0",
        "[method]pollable.ready",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/poll@0.2.0",
        "[method]pollable.block",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/poll@0.2.0",
        "[resource-drop]pollable",
        &StubRequirement::Required,
    ),
    ("wasi:io/poll@0.2.0", "poll", &StubRequirement::Required),
];

/// Replace imported WASI functions that implement general I/O access with no-ops
pub(crate) fn stub_io_virt(module: &mut Module) -> Result<()> {
    // Replace the I/O functions that are allowed to be missing
    for (module_name, func_name, stub_requirement) in WASI_IO_IMPORTS {
        match stub_requirement {
            // If the stub is always required we *must* find the function and replace it
            StubRequirement::Required => {
                let fid = module.imports.get_func(module_name, func_name)
                    .with_context(|| format!("failed to find required io import [{func_name}] in module [{module_name}]"))?;

                module
                    .replace_imported_func(fid, |(body, _)| {
                        body.unreachable();
                    })
                    .with_context(|| {
                        "failed to stub filesystem functionality [{}] in module [{export_name}]"
                    })?;
            }
            // If the stub is optional, we can replace the functions optimistically, and not fail if they are missing
            StubRequirement::Optional => {
                if let Ok(fid) = module.imports.get_func(module_name, func_name) {
                    module
                        .replace_imported_func(fid, |(body, _)| {
                            body.unreachable();
                        })
                        .with_context(|| {
                            "failed to stub filesystem functionality [{}] in module [{export_name}]"
                        })?;
                }
            }
            _ => bail!("unexpected stub requirement in imports for WASI I/O"),
        };
    }

    Ok(())
}

/// Exported functions related to IO
const WASI_IO_EXPORTS: &[&str] = &[
    "wasi:io/streams@0.2.0#[method]output-stream.blocking-flush",
    "wasi:io/streams@0.2.0#[method]input-stream.blocking-read",
    "wasi:io/streams@0.2.0#[method]input-stream.blocking-skip",
    "wasi:io/streams@0.2.0#[method]output-stream.blocking-splice",
    "wasi:io/streams@0.2.0#[method]output-stream.blocking-write-and-flush",
    "wasi:io/streams@0.2.0#[method]output-stream.check-write",
    "wasi:io/streams@0.2.0#[dtor]input-stream",
    "wasi:io/streams@0.2.0#[dtor]output-stream",
    "wasi:io/error@0.2.0#[dtor]error",
    "wasi:io/error@0.2.0#[method]error.to-debug-string",
    "wasi:io/streams@0.2.0#[method]output-stream.flush",
    "wasi:io/streams@0.2.0#[method]input-stream.read",
    "wasi:io/streams@0.2.0#[method]input-stream.skip",
    "wasi:io/streams@0.2.0#[method]output-stream.splice",
    "wasi:io/streams@0.2.0#[method]input-stream.subscribe",
    "wasi:io/streams@0.2.0#[method]output-stream.subscribe",
    "wasi:io/streams@0.2.0#[method]output-stream.write",
    "wasi:io/streams@0.2.0#[method]output-stream.write-zeroes",
    "wasi:io/streams@0.2.0#[method]output-stream.blocking-write-zeroes-and-flush",
    "wasi:io/poll@0.2.0#[method]pollable.ready",
    "wasi:io/poll@0.2.0#[method]pollable.block",
    "wasi:io/poll@0.2.0#[dtor]pollable",
    "wasi:io/poll@0.2.0#poll",
];

/// Strip exported WASI functions that implement IO (streams, polling) access
pub(crate) fn strip_io_virt(module: &mut Module) -> Result<()> {
    stub_io_virt(module)?;
    for &export_name in WASI_IO_EXPORTS {
        module.exports.remove(export_name).with_context(|| {
            format!("failed to strip general I/O export function [{export_name}]")
        })?;
    }
    Ok(())
}
