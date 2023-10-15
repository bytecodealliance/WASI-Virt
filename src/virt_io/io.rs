use anyhow::{bail, Context, Result};
use walrus::Module;

use super::StubRequirement;

/// Imports exposed by WASI for IO functionality
///
/// Some imports are required, and others are optional.
const WASI_IO_IMPORTS: [(&str, &str, &StubRequirement); 19] = [
    (
        "wasi:io/streams",
        "blocking-flush",
        &StubRequirement::Optional,
    ),
    (
        "wasi:io/streams",
        "blocking-read",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/streams",
        "blocking-skip",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/streams",
        "blocking-splice",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/streams",
        "blocking-write-and-flush",
        &StubRequirement::Optional,
    ),
    ("wasi:io/streams", "check-write", &StubRequirement::Optional),
    (
        "wasi:io/streams",
        "drop-input-stream",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/streams",
        "drop-output-stream",
        &StubRequirement::Required,
    ),
    ("wasi:io/streams", "flush", &StubRequirement::Optional),
    ("wasi:io/streams", "forward", &StubRequirement::Required),
    ("wasi:io/streams", "read", &StubRequirement::Optional),
    ("wasi:io/streams", "skip", &StubRequirement::Required),
    ("wasi:io/streams", "splice", &StubRequirement::Required),
    (
        "wasi:io/streams",
        "subscribe-to-input-stream",
        &StubRequirement::Required,
    ),
    (
        "wasi:io/streams",
        "subscribe-to-output-stream",
        &StubRequirement::Required,
    ),
    ("wasi:io/streams", "write", &StubRequirement::Required),
    (
        "wasi:io/streams",
        "write-zeroes",
        &StubRequirement::Required,
    ),
    (
        "wasi:poll/poll",
        "drop-pollable",
        &StubRequirement::Required,
    ),
    ("wasi:poll/poll", "poll-oneoff", &StubRequirement::Required),
];

/// Replace imported WASI functions that implement general I/O access with no-ops
pub(crate) fn stub_io_virt(module: &mut Module) -> Result<()> {
    // Replace the I/O functions that are allowed to be missing
    for (module_name, func_name, stub_requirement) in WASI_IO_IMPORTS {
        match stub_requirement {
            // If the stub is always required we *must* find the function and replace it
            StubRequirement::Required => {
                let fid = module.imports.get_func(module_name, func_name)
                    .with_context(|| format!("failed to find required filesystem import [{func_name}] in module [{module_name}]"))?;

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
const WASI_IO_EXPORTS: [&str; 19] = [
    "wasi:io/streams#blocking-flush",
    "wasi:io/streams#blocking-read",
    "wasi:io/streams#blocking-skip",
    "wasi:io/streams#blocking-splice",
    "wasi:io/streams#blocking-write-and-flush",
    "wasi:io/streams#check-write",
    "wasi:io/streams#drop-input-stream",
    "wasi:io/streams#drop-output-stream",
    "wasi:io/streams#flush",
    "wasi:io/streams#forward",
    "wasi:io/streams#read",
    "wasi:io/streams#skip",
    "wasi:io/streams#splice",
    "wasi:io/streams#subscribe-to-input-stream",
    "wasi:io/streams#subscribe-to-output-stream",
    "wasi:io/streams#write",
    "wasi:io/streams#write-zeroes",
    "wasi:poll/poll#drop-pollable",
    "wasi:poll/poll#poll-oneoff",
];

/// Strip exported WASI functions that implement IO (streams, polling) access
pub(crate) fn strip_io_virt(module: &mut Module) -> Result<()> {
    stub_io_virt(module)?;
    for export_name in WASI_IO_EXPORTS {
        module.exports.remove(export_name).with_context(|| {
            format!("failed to strip general I/O export function [{export_name}]")
        })?;
    }
    Ok(())
}
