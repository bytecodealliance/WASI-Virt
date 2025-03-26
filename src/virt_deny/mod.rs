use anyhow::Result;
use walrus::{ExportItem, FuncParams, FuncResults, FunctionBuilder, Module};

mod clocks;
mod exit;
mod http;
mod random;
mod sockets;
pub(crate) use clocks::deny_clocks_virt;
pub(crate) use exit::deny_exit_virt;
pub(crate) use http::deny_http_virt;
pub(crate) use random::deny_random_virt;
pub(crate) use sockets::deny_sockets_virt;

/// Helper function for replacing or inserting exports with stub functions
fn replace_or_insert_stub_for_exports<'a>(
    module: &mut Module,
    exports: impl IntoIterator<Item = &'a (&'a str, FuncParams, FuncResults)>,
) -> Result<()> {
    for (export_name, params, results) in exports {
        // TODO: look through all functions that are exported, and replace as we, finding varying versions

        // If the export exists, replace it directly
        if let Ok(fid) = module.exports.get_func(&export_name) {
            module.replace_exported_func(fid, |(body, _)| {
                body.unreachable();
            })?;
            continue;
        }

        // Create and use a new stub function for the export
        let mut builder = FunctionBuilder::new(&mut module.types, &params, &results);
        let mut body = builder.func_body();
        body.unreachable();
        module.exports.add(
            &export_name,
            ExportItem::Function(module.funcs.add_local(builder.local_func(vec![]))),
        );
    }
    Ok(())
}
