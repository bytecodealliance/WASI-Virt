use anyhow::Result;
use semver::Version;
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

use crate::WITInterfaceNameParts;

/// Version prefix that allows wasi version
const WASI_VERSION_PREFIX: &str = "0.2";

/// Helper function for replacing or inserting exports with stub functions
fn replace_or_insert_stub_for_exports<'a>(
    module: &mut Module,
    exports: impl IntoIterator<Item = &'a (WITInterfaceNameParts, FuncParams, FuncResults)>,
    insert_wasi_version: &Version,
) -> Result<()> {
    for ((ns, pkg, iface, export), params, results) in exports {
        let export_prefix = format!("{ns}:{pkg}/{iface}@{WASI_VERSION_PREFIX}");
        let export_suffix = format!("#{export}");

        let matching_export_fids = {
            module
                .exports
                .iter()
                .filter_map(|expt| match expt.item {
                    ExportItem::Function(fid)
                        if expt.name.starts_with(&export_prefix)
                            && expt.name.ends_with(&export_suffix) =>
                    {
                        Some(fid)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };

        if matching_export_fids.is_empty() {
            // Create and use a new stub function for the export
            let mut builder = FunctionBuilder::new(&mut module.types, &params, &results);
            let mut body = builder.func_body();
            let export_name = format!("{ns}:{pkg}/{iface}@{insert_wasi_version}#{export}");
            body.unreachable();
            module.exports.add(
                &export_name,
                ExportItem::Function(module.funcs.add_local(builder.local_func(vec![]))),
            );
            eprintln!("EXPORT {}", export_name);
        }

        for fid in matching_export_fids.iter() {
            module.replace_exported_func(fid.clone(), |(body, _)| {
                body.unreachable();
            })?;
        }
    }
    Ok(())
}
