use std::sync::OnceLock;

use anyhow::Result;
use walrus::{FuncParams, FuncResults, Module, ValType};

use crate::walrus_ops::stub_virt;

use super::replace_or_insert_stub_for_exports;

/// Functions that represent the HTTP functionality provided by WASI https
pub(crate) static WASI_HTTP_FNS: OnceLock<Vec<(&str, FuncParams, FuncResults)>> = OnceLock::new();

/// Retrieve or initialize the static list of functions related to HTTP in WASI
fn get_wasi_http_fns() -> &'static Vec<(&'static str, FuncParams, FuncResults)> {
    WASI_HTTP_FNS.get_or_init(|| {
        Vec::from([
            (
                "wasi:http/incoming-handler@0.2.0#handle",
                vec![ValType::I32, ValType::I32],
                vec![],
            ),
            (
                "wasi:http/outgoing-handler@0.2.0#handle",
                vec![
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                ],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[dtor]fields",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types@0.2.0#[constructor]fields",
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[constructor]fields.from-list",
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]fields.get",
                vec![ValType::I32, ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]fields.has",
                vec![ValType::I32, ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]fields.set",
                vec![
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                ],
                vec![],
            ),
            (
                "wasi:http/types@0.2.0#[method]fields.delete",
                vec![ValType::I32, ValType::I32, ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types@0.2.0#[method]fields.append",
                vec![
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                ],
                vec![],
            ),
            (
                "wasi:http/types@0.2.0#[method]fields.entries",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]fields.clone",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[dtor]incoming-request",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types@0.2.0#[method]incoming-request.method",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]incoming-request.path-with-query",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]incoming-request.scheme",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]incoming-request.authority",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]incoming-request.headers",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]incoming-request.consume",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[dtor]outgoing-request",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types@0.2.0#[constructor]outgoing-request",
                vec![
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                ],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]outgoing-request.body",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]outgoing-request.method",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]outgoing-request.set-method",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]outgoing-request.path-with-query",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]outgoing-request.set-path-with-query",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]outgoing-request.scheme",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]outgoing-request.set-scheme",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]outgoing-request.authority",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]outgoing-request.set-authority",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]outgoing-request.headers",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[dtor]incoming-body",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types@0.2.0#[method]incoming-body.stream",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[static]incoming-body.finish",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[dtor]outgoing-body",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types@0.2.0#[method]outgoing-body.write",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[static]outgoing-body.finish",
                vec![ValType::I32, ValType::I32, ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types@0.2.0#[dtor]response-outparam",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types@0.2.0#[static]response-outparam.set",
                vec![
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                    ValType::I32,
                ],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[dtor]incoming-response",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types@0.2.0#[method]incoming-response.status",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]incoming-response.headers",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]incoming-response.consume",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[dtor]outgoing-response",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types@0.2.0#[constructor]outgoing-response",
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]outgoing-response.body",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]outgoing-response.status-code",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]outgoing-response.set-status-code",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]outgoing-response.headers",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[dtor]future-incoming-response",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types@0.2.0#[method]future-incoming-response.get",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]future-incoming-response.subscribe",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[dtor]future-trailers",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types@0.2.0#[method]future-trailers.subscribe",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types@0.2.0#[method]future-trailers.get",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/outgoing-handler@0.2.0#handle",
                vec![ValType::I32; 8],
                vec![ValType::I32],
            ),
        ])
    })
}

/// Replace exports related to HTTP in WASI to deny access
pub(crate) fn deny_virt(module: &mut Module, subsystems: &[&str]) -> Result<()> {
    stub_virt(module, subsystems)?;
    let mut subsystem_exports = Vec::new();
    for export in module.exports.iter() {
        let export_name = if export.name.starts_with("cabi_post_") {
            &export.name[10..]
        } else {
            &export.name
        };
        if subsystems
            .iter()
            .any(|subsystem| export_name.starts_with(subsystem))
        {
            subsystem_exports.push(export.name.to_string());
        }
    }
    for export_name in &subsystem_exports {
        let fid = module.exports.get_func(export_name).unwrap();
        module.replace_exported_func(fid, |(body, _)| {
            body.unreachable();
        })?;
    }
    Ok(())
}
