use std::sync::OnceLock;

use anyhow::Result;
use walrus::{FuncParams, FuncResults, Module, ValType};

use crate::virt_io::stub_http_virt;

use super::replace_or_insert_stub_for_exports;

/// Functions that represent the HTTP functionality provided by WASI https
pub(crate) static WASI_HTTP_FNS: OnceLock<Vec<(&str, FuncParams, FuncResults)>> = OnceLock::new();

/// Retrieve or initialize the static list of functions related to HTTP in WASI
fn get_wasi_http_fns() -> &'static Vec<(&'static str, FuncParams, FuncResults)> {
    WASI_HTTP_FNS.get_or_init(|| {
        Vec::from([
            (
                "wasi:http/incoming-handler#handle",
                vec![ValType::I32, ValType::I32],
                vec![],
            ),
            (
                "wasi:http/outgoing-handler#handle",
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
            ("wasi:http/types#drop-fields", vec![ValType::I32], vec![]),
            (
                "wasi:http/types#new-fields",
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types#fields-get",
                vec![ValType::I32, ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types#fields-set",
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
                "wasi:http/types#fields-delete",
                vec![ValType::I32, ValType::I32, ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types#fields-append",
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
                "wasi:http/types#fields-entries",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types#fields-clone",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types#finish-incoming-stream",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types#finish-outgoing-stream",
                vec![ValType::I32, ValType::I32, ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types#drop-incoming-request",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types#drop-outgoing-request",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types#incoming-request-method",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types#incoming-request-path-with-query",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types#incoming-request-scheme",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types#incoming-request-authority",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types#incoming-request-headers",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types#incoming-request-consume",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types#new-outgoing-request",
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
                "wasi:http/types#outgoing-request-write",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types#drop-response-outparam",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types#set-response-outparam",
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
                "wasi:http/types#drop-incoming-response",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types#drop-outgoing-response",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types#incoming-response-status",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types#incoming-response-headers",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types#incoming-response-consume",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types#new-outgoing-response",
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types#outgoing-response-write",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types#drop-future-incoming-response",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:http/types#future-incoming-response-get",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:http/types#listen-to-future-incoming-response",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
        ])
    })
}

/// Replace exports related to HTTP in WASI to deny access
pub(crate) fn deny_http_virt(module: &mut Module) -> Result<()> {
    stub_http_virt(module)?;
    replace_or_insert_stub_for_exports(module, get_wasi_http_fns())
}
