use anyhow::{bail, Context, Result};
use walrus::Module;

use super::StubRequirement;

/// Exported functions related to WASI http
const WASI_HTTP_EXPORTS: &[&str] = &[
    "wasi:http/types@0.2.0#[dtor]fields",
    "wasi:http/types@0.2.0#[constructor]fields",
    "wasi:http/types@0.2.0#[method]fields.get",
    "wasi:http/types@0.2.0#[method]fields.set",
    "wasi:http/types@0.2.0#[method]fields.delete",
    "wasi:http/types@0.2.0#[method]fields.append",
    "wasi:http/types@0.2.0#[method]fields.entries",
    "wasi:http/types@0.2.0#[method]fields.clone",
    "wasi:http/types@0.2.0#[method]incoming-body.stream",
    "wasi:http/types@0.2.0#[static]incoming-body.finish",
    "wasi:http/types@0.2.0#[static]outgoing-body.finish",
    "wasi:http/types@0.2.0#[dtor]incoming-request",
    "wasi:http/types@0.2.0#[dtor]incoming-body",
    "wasi:http/types@0.2.0#[dtor]outgoing-request",
    "wasi:http/types@0.2.0#[dtor]outgoing-body",
    "wasi:http/types@0.2.0#[method]incoming-request.method",
    "wasi:http/types@0.2.0#[method]incoming-request.path-with-query",
    "wasi:http/types@0.2.0#[method]incoming-request.scheme",
    "wasi:http/types@0.2.0#[method]incoming-request.authority",
    "wasi:http/types@0.2.0#[method]incoming-request.headers",
    "wasi:http/types@0.2.0#[method]incoming-request.consume",
    "wasi:http/types@0.2.0#[constructor]outgoing-request",
    "wasi:http/types@0.2.0#[method]outgoing-request.write",
    "wasi:http/types@0.2.0#[dtor]response-outparam",
    "wasi:http/types@0.2.0#[static]response-outparam.set",
    "wasi:http/types@0.2.0#[dtor]incoming-response",
    "wasi:http/types@0.2.0#[dtor]outgoing-response",
    "wasi:http/types@0.2.0#[method]incoming-response.status",
    "wasi:http/types@0.2.0#[method]incoming-response.headers",
    "wasi:http/types@0.2.0#[method]incoming-response.consume",
    "wasi:http/types@0.2.0#[constructor]outgoing-response",
    "wasi:http/types@0.2.0#[method]outgoing-response.write",
    "wasi:http/types@0.2.0#[method]outgoing-body.write",
    "wasi:http/types@0.2.0#[dtor]future-trailers",
    "wasi:http/types@0.2.0#[method]future-trailers.get",
    "wasi:http/types@0.2.0#[method]future-trailers.subscribe",
    "wasi:http/types@0.2.0#[dtor]future-incoming-response",
    "wasi:http/types@0.2.0#[method]future-incoming-response.get",
    "wasi:http/types@0.2.0#[method]future-incoming-response.subscribe",
    "wasi:http/outgoing-handler@0.2.0#handle",
];

/// Strip exported WASI functions that implement HTTP access
pub(crate) fn strip_http_virt(module: &mut Module) -> Result<()> {
    stub_http_virt(module)?;
    for &export_name in WASI_HTTP_EXPORTS {
        module
            .exports
            .remove(export_name)
            .with_context(|| format!("failed to strip WASI HTTP function [{export_name}]"))?;
    }
    Ok(())
}

/// Imports exposed by WASI for HTTP functionality
const WASI_HTTP_IMPORTS: [(&str, &str, &StubRequirement); 32] = [
    ("wasi:http/types", "drop-fields", &StubRequirement::Optional),
    ("wasi:http/types", "new-fields", &StubRequirement::Optional),
    ("wasi:http/types", "fields-get", &StubRequirement::Optional),
    ("wasi:http/types", "fields-set", &StubRequirement::Optional),
    (
        "wasi:http/types",
        "fields-delete",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "fields-append",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "fields-entries",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "fields-clone",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "finish-incoming-stream",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "finish-outgoing-stream",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "drop-incoming-request",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "drop-outgoing-request",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "incoming-request-method",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "incoming-request-path-with-query",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "incoming-request-scheme",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "incoming-request-authority",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "incoming-request-headers",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "incoming-request-consume",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "new-outgoing-request",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "outgoing-request-write",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "drop-response-outparam",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "set-response-outparam",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "drop-incoming-response",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "drop-outgoing-response",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "incoming-response-status",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "incoming-response-headers",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "incoming-response-consume",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "new-outgoing-response",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "outgoing-response-write",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "drop-future-incoming-response",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "future-incoming-response-get",
        &StubRequirement::Optional,
    ),
    (
        "wasi:http/types",
        "listen-to-future-incoming-response",
        &StubRequirement::Optional,
    ),
];

/// Replace imported WASI functions that implement HTTP access with no-ops
pub(crate) fn stub_http_virt(module: &mut Module) -> Result<()> {
    for (module_name, func_name, stub_requirement) in WASI_HTTP_IMPORTS {
        match stub_requirement {
            StubRequirement::Optional => {
                if let Ok(fid) = module.imports.get_func(module_name, func_name) {
                    module
                        .replace_imported_func(fid, |(body, _)| {
                            body.unreachable();
                        })
                        .with_context(|| {
                            "failed to stub WASI HTTP functionality [{}] in module [{export_name}]"
                        })?;
                }
            }
            _ => bail!("unexpected stub requirement in imports for WASI HTTP"),
        }
    }
    Ok(())
}
