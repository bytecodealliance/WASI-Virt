use anyhow::{bail, Context, Result};
use walrus::Module;

use super::StubRequirement;

/// Imports exposed by WASI for clocks functionality which are allowed to be
const WASI_CLOCKS_IMPORTS: [(&str, &str, &StubRequirement); 3] = [
    (
        "wasi:clocks/monotonic-clock@0.2.0-rc-2023-10-18",
        "now",
        &StubRequirement::Required,
    ),
    (
        "wasi:clocks/monotonic-clock@0.2.0-rc-2023-10-18",
        "resolution",
        &StubRequirement::Required,
    ),
    (
        "wasi:clocks/monotonic-clock@0.2.0-rc-2023-10-18",
        "subscribe",
        &StubRequirement::Required,
    ),
];

/// Replace imported WASI functions that implement clocks access with no-ops
pub(crate) fn stub_clocks_virt(module: &mut Module) -> Result<()> {
    for (module_name, func_name, stub_requirement) in WASI_CLOCKS_IMPORTS {
        match stub_requirement {
            StubRequirement::Required => {
                let fid = module
                    .imports
                    .get_func(module_name, func_name)
                    .with_context(|| {
                        format!(
                            "failed to find required clocks import [{func_name}] in module [{module_name}]"
                        )
                    })?;
                module
                    .replace_imported_func(fid, |(body, _)| {
                        body.unreachable();
                    })
                    .with_context(|| {
                        "failed to stub clocks functionality [{}] in module [{export_name}]"
                    })?;
            }
            _ => bail!("unexpected stub requirement in imports for WASI clocks"),
        }
    }
    Ok(())
}

/// Exported functions related to WASI clocks
const WASI_CLOCK_EXPORTS: [&str; 3] = [
    "wasi:clocks/monotonic-clock@0.2.0-rc-2023-10-18#now",
    "wasi:clocks/monotonic-clock@0.2.0-rc-2023-10-18#resolution",
    "wasi:clocks/monotonic-clock@0.2.0-rc-2023-10-18#subscribe",
];

/// Strip exported WASI functions that implement clock access
pub(crate) fn strip_clocks_virt(module: &mut Module) -> Result<()> {
    stub_clocks_virt(module)?;
    for export_name in WASI_CLOCK_EXPORTS {
        module
            .exports
            .remove(export_name)
            .with_context(|| format!("failed to strip WASI clocks function [{export_name}]"))?;
    }
    Ok(())
}
