use std::sync::OnceLock;

use anyhow::Result;
use semver::Version;
use walrus::{FuncParams, FuncResults, Module, ValType};

use crate::walrus_ops::stub_virt;

use super::replace_or_insert_stub_for_exports;
use crate::WITInterfaceNameParts;

/// Functions that represent the environment functionality provided by WASI sockets
static WASI_SOCKETS_FNS: OnceLock<Vec<(WITInterfaceNameParts, FuncParams, FuncResults)>> =
    OnceLock::new();

/// Retrieve or initialize the static list of functions related to sockets in WASI
pub fn get_wasi_sockets_fns() -> &'static Vec<(WITInterfaceNameParts, FuncParams, FuncResults)> {
    WASI_SOCKETS_FNS.get_or_init(|| {
        Vec::from([
            (
                &("wasi", "sockets", "network", "drop-network"),
                vec![ValType::I32],
                vec![],
            ),
            (
                &("wasi", "sockets", "instance-network", "instance-network"),
                vec![],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "ip-name-lookup", "resolve-addresses"),
                vec![ValType::I32, ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "ip-name-lookup", "resolve-next-address"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &(
                    "wasi",
                    "sockets",
                    "ip-name-lookup",
                    "drop-resolve-address-stream",
                ),
                vec![ValType::I32],
                vec![],
            ),
            (
                &("wasi", "sockets", "ip-name-lookup", "subscribe"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "tcp-create-socket", "create-tcp-socket"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "tcp", "start-bind"),
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
                &("wasi", "sockets", "tcp", "finish-bind"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "tcp", "start-connect"),
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
                &("wasi", "sockets", "tcp", "finish-connect"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "tcp", "start-listen"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "tcp", "finish-listen"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "tcp", "accept"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "tcp", "local-address"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "tcp", "remote-address"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "tcp", "[method]tcp-socket.is-listening"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "tcp", "address-family"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "tcp", "set-listen-backlog-size"),
                vec![ValType::I32, ValType::I64],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "tcp", "keep-alive-enabled"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "tcp", "set-keep-alive-enabled"),
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "tcp", "keep-alive-idle-time"),
                vec![ValType::I32, ValType::I32],
                vec![],
            ),
            (
                &("wasi", "sockets", "tcp", "set-keep-alive-idle-time"),
                vec![ValType::I32, ValType::I64, ValType::I32],
                vec![],
            ),
            (
                &("wasi", "sockets", "tcp", "keep-alive-interval"),
                vec![ValType::I32, ValType::I32],
                vec![],
            ),
            (
                &("wasi", "sockets", "tcp", "set-keep-alive-interval"),
                vec![ValType::I32, ValType::I64, ValType::I32],
                vec![],
            ),
            (
                &("wasi", "sockets", "tcp", "keep-alive-count"),
                vec![ValType::I32, ValType::I32],
                vec![],
            ),
            (
                &("wasi", "sockets", "tcp", "set-keep-alive-count"),
                vec![ValType::I32, ValType::I32, ValType::I32],
                vec![],
            ),
            (
                &("wasi", "sockets", "tcp", "hop-limit"),
                vec![ValType::I32, ValType::I32],
                vec![],
            ),
            (
                &("wasi", "sockets", "tcp", "set-hop-limit"),
                vec![ValType::I32, ValType::I32, ValType::I32],
                vec![],
            ),
            (
                &("wasi", "sockets", "tcp", "receive-buffer-size"),
                vec![ValType::I32, ValType::I32],
                vec![],
            ),
            (
                &("wasi", "sockets", "tcp", "set-receive-buffer-size"),
                vec![ValType::I32, ValType::I64, ValType::I32],
                vec![],
            ),
            (
                &("wasi", "sockets", "tcp", "send-buffer-size"),
                vec![ValType::I32, ValType::I32],
                vec![],
            ),
            (
                &("wasi", "sockets", "tcp", "set-send-buffer-size"),
                vec![ValType::I32, ValType::I64, ValType::I32],
                vec![],
            ),
            (
                &("wasi", "sockets", "tcp", "subscribe"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "tcp", "shutdown"),
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "tcp", "drop-tcp-socket"),
                vec![ValType::I32],
                vec![],
            ),
            (
                &("wasi", "sockets", "udp-create-socket", "create-udp-socket"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "udp", "start-bind"),
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
                &("wasi", "sockets", "udp", "finish-bind"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "udp", "local-address"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "udp", "remote-address"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "udp", "address-family"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "udp", "unicast-hop-limit"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "udp", "set-unicast-hop-limit"),
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "udp", "receive-buffer-size"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "udp", "set-receive-buffer-size"),
                vec![ValType::I32, ValType::I64],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "udp", "send-buffer-size"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "udp", "set-send-buffer-size"),
                vec![ValType::I32, ValType::I64],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "udp", "subscribe"),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &("wasi", "sockets", "udp", "drop-udp-socket"),
                vec![ValType::I32],
                vec![],
            ),
            (
                &("wasi", "sockets", "udp", "[method]udp-socket.stream"),
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
                    ValType::I32,
                ],
                vec![],
            ),
            (
                &(
                    "wasi",
                    "sockets",
                    "udp",
                    "[method]incoming-datagram-stream.receive",
                ),
                vec![ValType::I32, ValType::I64, ValType::I32],
                vec![],
            ),
            (
                &(
                    "wasi",
                    "sockets",
                    "udp",
                    "[method]incoming-datagram-stream.subscribe",
                ),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &(
                    "wasi",
                    "sockets",
                    "udp",
                    "[resource-drop]incoming-datagram-stream",
                ),
                vec![ValType::I32],
                vec![],
            ),
            (
                &(
                    "wasi",
                    "sockets",
                    "udp",
                    "[method]outgoing-datagram-stream.check-send",
                ),
                vec![ValType::I32, ValType::I32],
                vec![],
            ),
            (
                &(
                    "wasi",
                    "sockets",
                    "udp",
                    "[method]outgoing-datagram-stream.send",
                ),
                vec![ValType::I32, ValType::I32, ValType::I32, ValType::I32],
                vec![],
            ),
            (
                &(
                    "wasi",
                    "sockets",
                    "udp",
                    "[method]outgoing-datagram-stream.subscribe",
                ),
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                &(
                    "wasi",
                    "sockets",
                    "udp",
                    "[resource-drop]outgoing-datagram-stream",
                ),
                vec![ValType::I32],
                vec![],
            ),
        ])
    })
}

/// Replace exports related to sockets in WASI to deny access
///
/// # Arguments
///
/// * `module` - The module to deny
/// * `insert_wasi_version` - version of WASI to use when inserting stubs
///
pub(crate) fn deny_sockets_virt(
    module: &mut Module,
    insert_wasi_version: &Version,
) -> Result<()> {
    stub_virt(module, &["wasi:sockets/"], false)?;
    replace_or_insert_stub_for_exports(module, get_wasi_sockets_fns(), insert_wasi_version)
}
