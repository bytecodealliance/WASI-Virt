use anyhow::{bail, Context, Result};
use walrus::Module;

use super::StubRequirement;

/// Imports exposed by WASI for sockets functionality which are allowed to be missing
const WASI_SOCKETS_IMPORTS: [(&str, &str, &StubRequirement); 49] = [
    (
        "wasi:sockets/ip-name-lookup",
        "resolve-addresses",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/ip-name-lookup",
        "resolve-next-address",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/ip-name-lookup",
        "drop-resolve-address-stream",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/ip-name-lookup",
        "subscribe",
        &StubRequirement::Required,
    ),
    ("wasi:sockets/tcp", "start-bind", &StubRequirement::Required),
    (
        "wasi:sockets/tcp",
        "finish-bind",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/tcp",
        "start-connect",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/tcp",
        "finish-connect",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/tcp",
        "start-listen",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/tcp",
        "finish-listen",
        &StubRequirement::Required,
    ),
    ("wasi:sockets/tcp", "accept", &StubRequirement::Required),
    (
        "wasi:sockets/tcp",
        "local-address",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/tcp",
        "remote-address",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/tcp",
        "address-family",
        &StubRequirement::Required,
    ),
    ("wasi:sockets/tcp", "ipv6-only", &StubRequirement::Required),
    (
        "wasi:sockets/tcp",
        "set-ipv6-only",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/tcp",
        "set-listen-backlog-size",
        &StubRequirement::Required,
    ),
    ("wasi:sockets/tcp", "keep-alive", &StubRequirement::Required),
    (
        "wasi:sockets/tcp",
        "set-keep-alive",
        &StubRequirement::Required,
    ),
    ("wasi:sockets/tcp", "no-delay", &StubRequirement::Required),
    (
        "wasi:sockets/tcp",
        "set-no-delay",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/tcp",
        "unicast-hop-limit",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/tcp",
        "set-unicast-hop-limit",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/tcp",
        "receive-buffer-size",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/tcp",
        "set-receive-buffer-size",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/tcp",
        "send-buffer-size",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/tcp",
        "set-send-buffer-size",
        &StubRequirement::Required,
    ),
    ("wasi:sockets/tcp", "subscribe", &StubRequirement::Required),
    ("wasi:sockets/tcp", "shutdown", &StubRequirement::Required),
    (
        "wasi:sockets/tcp",
        "drop-tcp-socket",
        &StubRequirement::Required,
    ),
    ("wasi:sockets/udp", "start-bind", &StubRequirement::Required),
    (
        "wasi:sockets/udp",
        "finish-bind",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/udp",
        "start-connect",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/udp",
        "finish-connect",
        &StubRequirement::Required,
    ),
    ("wasi:sockets/udp", "receive", &StubRequirement::Required),
    ("wasi:sockets/udp", "send", &StubRequirement::Required),
    (
        "wasi:sockets/udp",
        "local-address",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/udp",
        "remote-address",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/udp",
        "address-family",
        &StubRequirement::Required,
    ),
    ("wasi:sockets/udp", "ipv6-only", &StubRequirement::Required),
    (
        "wasi:sockets/udp",
        "set-ipv6-only",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/udp",
        "unicast-hop-limit",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/udp",
        "set-unicast-hop-limit",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/udp",
        "receive-buffer-size",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/udp",
        "set-receive-buffer-size",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/udp",
        "send-buffer-size",
        &StubRequirement::Required,
    ),
    (
        "wasi:sockets/udp",
        "set-send-buffer-size",
        &StubRequirement::Required,
    ),
    ("wasi:sockets/udp", "subscribe", &StubRequirement::Required),
    (
        "wasi:sockets/udp",
        "drop-udp-socket",
        &StubRequirement::Required,
    ),
];

/// Replace imported WASI functions that implement socket access with no-ops
pub(crate) fn stub_sockets_virt(module: &mut Module) -> Result<()> {
    for (module_name, func_name, stub_requirement) in WASI_SOCKETS_IMPORTS {
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
            _ => bail!("unexpected stub requirement in imports for WASI sockets"),
        }
    }

    Ok(())
}

/// Exported functions related to sockets
const WASI_SOCKETS_EXPORTS: [&str; 49] = [
    "wasi:sockets/ip-name-lookup#resolve-addresses",
    "wasi:sockets/ip-name-lookup#resolve-next-address",
    "wasi:sockets/ip-name-lookup#drop-resolve-address-stream",
    "wasi:sockets/ip-name-lookup#subscribe",
    "wasi:sockets/tcp#start-bind",
    "wasi:sockets/tcp#finish-bind",
    "wasi:sockets/tcp#start-connect",
    "wasi:sockets/tcp#finish-connect",
    "wasi:sockets/tcp#start-listen",
    "wasi:sockets/tcp#finish-listen",
    "wasi:sockets/tcp#accept",
    "wasi:sockets/tcp#local-address",
    "wasi:sockets/tcp#remote-address",
    "wasi:sockets/tcp#address-family",
    "wasi:sockets/tcp#ipv6-only",
    "wasi:sockets/tcp#set-ipv6-only",
    "wasi:sockets/tcp#set-listen-backlog-size",
    "wasi:sockets/tcp#keep-alive",
    "wasi:sockets/tcp#set-keep-alive",
    "wasi:sockets/tcp#no-delay",
    "wasi:sockets/tcp#set-no-delay",
    "wasi:sockets/tcp#unicast-hop-limit",
    "wasi:sockets/tcp#set-unicast-hop-limit",
    "wasi:sockets/tcp#receive-buffer-size",
    "wasi:sockets/tcp#set-receive-buffer-size",
    "wasi:sockets/tcp#send-buffer-size",
    "wasi:sockets/tcp#set-send-buffer-size",
    "wasi:sockets/tcp#subscribe",
    "wasi:sockets/tcp#shutdown",
    "wasi:sockets/tcp#drop-tcp-socket",
    "wasi:sockets/udp#start-bind",
    "wasi:sockets/udp#finish-bind",
    "wasi:sockets/udp#start-connect",
    "wasi:sockets/udp#finish-connect",
    "wasi:sockets/udp#receive",
    "wasi:sockets/udp#send",
    "wasi:sockets/udp#local-address",
    "wasi:sockets/udp#remote-address",
    "wasi:sockets/udp#address-family",
    "wasi:sockets/udp#ipv6-only",
    "wasi:sockets/udp#set-ipv6-only",
    "wasi:sockets/udp#unicast-hop-limit",
    "wasi:sockets/udp#set-unicast-hop-limit",
    "wasi:sockets/udp#receive-buffer-size",
    "wasi:sockets/udp#set-receive-buffer-size",
    "wasi:sockets/udp#send-buffer-size",
    "wasi:sockets/udp#set-send-buffer-size",
    "wasi:sockets/udp#subscribe",
    "wasi:sockets/udp#drop-udp-socket",
];

/// Strip exported WASI functions that implement sockets access
pub(crate) fn strip_sockets_virt(module: &mut Module) -> Result<()> {
    stub_sockets_virt(module)?;
    for export_name in WASI_SOCKETS_EXPORTS {
        module.exports.remove(export_name).with_context(|| {
            format!("failed to strip WASI sockets export function [{export_name}]")
        })?;
    }
    Ok(())
}
