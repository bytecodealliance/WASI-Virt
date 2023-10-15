use anyhow::{bail, Context, Result};
use walrus::Module;

use super::StubRequirement;

/// Imports exposed by WASI for STDIO functionality which are allowed to be missing
const WASI_STDIO_IMPORTS: [(&str, &str, &StubRequirement); 8] = [
    ("wasi:cli/stdin", "get-stdin", &StubRequirement::Optional),
    ("wasi:cli/stdout", "get-stdout", &StubRequirement::Optional),
    ("wasi:cli/stderr", "get-stderr", &StubRequirement::Optional),
    (
        "wasi:cli/terminal-stdin",
        "get-terminal-stdin",
        &StubRequirement::Optional,
    ),
    (
        "wasi:cli/terminal-stdout",
        "get-terminal-stdout",
        &StubRequirement::Optional,
    ),
    (
        "wasi:cli/terminal-stderr",
        "get-terminal-stderr",
        &StubRequirement::Optional,
    ),
    (
        "wasi:cli/terminal-input",
        "drop-terminal-input",
        &StubRequirement::Optional,
    ),
    (
        "wasi:cli/terminal-output",
        "drop-terminal-output",
        &StubRequirement::Optional,
    ),
];

/// Replace imported WASI functions that implement STDIO access with no-ops
pub(crate) fn stub_stdio_virt(module: &mut Module) -> Result<()> {
    for (module_name, func_name, stub_requirement) in WASI_STDIO_IMPORTS {
        match stub_requirement {
            StubRequirement::Optional => {
                if let Ok(fid) = module.imports.get_func(module_name, func_name) {
                    module
                        .replace_imported_func(fid, |(body, _)| {
                            body.unreachable();
                        })
                        .with_context(|| {
                            format!(
                        "failed to stub STDIO functionality [{func_name}] in module [{module_name}]"
                    )
                        })?;
                }
            }
            _ => bail!("unexpected stub requirement in imports for WASI STD I/O"),
        }
    }
    Ok(())
}

/// Exported functions related to STDIO
const WASI_STDIO_EXPORTS: [&str; 8] = [
    "wasi:cli/stdin#get-stdin",
    "wasi:cli/stdout#get-stdout",
    "wasi:cli/stderr#get-stderr",
    "wasi:cli/terminal-stdin#get-terminal-stdin",
    "wasi:cli/terminal-stdout#get-terminal-stdout",
    "wasi:cli/terminal-stderr#get-terminal-stderr",
    "wasi:cli/terminal-input#drop-terminal-input",
    "wasi:cli/terminal-output#drop-terminal-output",
];

/// Strip exported WASI functions that implement standard I/O (stdin, stdout, etc) access
pub(crate) fn strip_stdio_virt(module: &mut Module) -> Result<()> {
    stub_stdio_virt(module)?;
    for export_name in WASI_STDIO_EXPORTS {
        module
            .exports
            .remove(export_name)
            .with_context(|| format!("failed to strip std I/O function [{export_name}]"))?;
    }
    Ok(())
}
