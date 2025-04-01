use std::sync::OnceLock;

use anyhow::Result;
use semver::Version;
use walrus::{FuncParams, FuncResults, Module, ValType};

use crate::walrus_ops::stub_virt;

use super::replace_or_insert_stub_for_exports;
use crate::WITInterfaceNameParts;

/// Functions that represent the HTTP functionality provided by WASI https
pub(crate) static WASI_HTTP_FNS: OnceLock<Vec<(WITInterfaceNameParts, FuncParams, FuncResults)>> =
    OnceLock::new();

/// Retrieve or initialize the static list of functions related to HTTP in WASI
fn get_wasi_http_fns() -> &'static Vec<(WITInterfaceNameParts, FuncParams, FuncResults)> {
    WASI_HTTP_FNS.get_or_init(|| {
        Vec::from([
            (
                &("wasi", "http", "incoming-handler", "handle"),
                vec![ValType::I32, ValType::I32],
                vec![],
            ),
            (
                &("wasi", "http", "outgoing-handler", "handle"),
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
                &("wasi", "http", "types", "[dtor]fields"),
                vec![ValType::I32],
                vec![],
            ),
            (
                &("wasi", "http", "types", "[constructor]fields"),
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[constructor]fields.from-list"),
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[method]fields.get"),
                vec![ValType::I32, ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[method]fields.has"),
                vec![ValType::I32, ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[method]fields.set"),
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
                &("wasi", "http", "types", "[method]fields.delete"),
                vec![ValType::I32, ValType::I32, ValType::I32],
                vec![],
            ),
            (
                &("wasi", "http", "types", "[method]fields.append"),
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
                &("wasi", "http", "types", "[method]fields.entries"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[method]fields.clone"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[dtor]incoming-request"),
                vec![ValType::I32],
                vec![],
            ),
            (
                &("wasi", "http", "types", "[method]incoming-request.method"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &(
                    "wasi",
                    "http",
                    "types",
                    "[method]incoming-request.path-with-query",
                ),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[method]incoming-request.scheme"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &(
                    "wasi",
                    "http",
                    "types",
                    "[method]incoming-request.authority",
                ),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[method]incoming-request.headers"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[method]incoming-request.consume"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[dtor]outgoing-request"),
                vec![ValType::I32],
                vec![],
            ),
            (
                &("wasi", "http", "types", "[constructor]outgoing-request"),
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
                &("wasi", "http", "types", "[method]outgoing-request.body"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[method]outgoing-request.method"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &(
                    "wasi",
                    "http",
                    "types",
                    "[method]outgoing-request.set-method",
                ),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &(
                    "wasi",
                    "http",
                    "types",
                    "[method]outgoing-request.path-with-query",
                ),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &(
                    "wasi",
                    "http",
                    "types",
                    "[method]outgoing-request.set-path-with-query",
                ),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[method]outgoing-request.scheme"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &(
                    "wasi",
                    "http",
                    "types",
                    "[method]outgoing-request.set-scheme",
                ),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &(
                    "wasi",
                    "http",
                    "types",
                    "[method]outgoing-request.authority",
                ),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &(
                    "wasi",
                    "http",
                    "types",
                    "[method]outgoing-request.set-authority",
                ),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[method]outgoing-request.headers"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[dtor]incoming-body"),
                vec![ValType::I32],
                vec![],
            ),
            (
                &("wasi", "http", "types", "[method]incoming-body.stream"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[static]incoming-body.finish"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[dtor]outgoing-body"),
                vec![ValType::I32],
                vec![],
            ),
            (
                &("wasi", "http", "types", "[method]outgoing-body.write"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[static]outgoing-body.finish"),
                vec![ValType::I32, ValType::I32, ValType::I32],
                vec![],
            ),
            (
                &("wasi", "http", "types", "[dtor]response-outparam"),
                vec![ValType::I32],
                vec![],
            ),
            (
                &("wasi", "http", "types", "[static]response-outparam.set"),
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
                &("wasi", "http", "types", "[dtor]incoming-response"),
                vec![ValType::I32],
                vec![],
            ),
            (
                &("wasi", "http", "types", "[method]incoming-response.status"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[method]incoming-response.headers"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[method]incoming-response.consume"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[dtor]outgoing-response"),
                vec![ValType::I32],
                vec![],
            ),
            (
                &("wasi", "http", "types", "[constructor]outgoing-response"),
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[method]outgoing-response.body"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &(
                    "wasi",
                    "http",
                    "types",
                    "[method]outgoing-response.status-code",
                ),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &(
                    "wasi",
                    "http",
                    "types",
                    "[method]outgoing-response.set-status-code",
                ),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[method]outgoing-response.headers"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[dtor]future-incoming-response"),
                vec![ValType::I32],
                vec![],
            ),
            (
                &(
                    "wasi",
                    "http",
                    "types",
                    "[method]future-incoming-response.get",
                ),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &(
                    "wasi",
                    "http",
                    "types",
                    "[method]future-incoming-response.subscribe",
                ),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[dtor]future-trailers"),
                vec![ValType::I32],
                vec![],
            ),
            (
                &("wasi", "http", "types", "[method]future-trailers.subscribe"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "types", "[method]future-trailers.get"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "http", "outgoing-handler", "handle"),
                vec![ValType::I32; 8],
                vec![ValType::I32],
            ),
        ])
    })
}

/// Replace exports related to HTTP in WASI to deny access
///
/// # Arguments
///
/// * `module` - The module to deny
/// * `insert_wasi_version` - version of WASI to use when inserting stubs
///
pub(crate) fn deny_http_virt(module: &mut Module, insert_wasi_version: &Version) -> Result<()> {
    stub_virt(module, &["wasi:http/"], false)?;
    replace_or_insert_stub_for_exports(module, get_wasi_http_fns(), insert_wasi_version)
}
