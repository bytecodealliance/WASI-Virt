use std::sync::OnceLock;

use anyhow::Result;
use walrus::{FuncParams, FuncResults, Module, ValType};

use crate::walrus_ops::stub_virt;

use super::replace_or_insert_stub_for_exports;

/// Functions that represent the environment functionality provided by WASI sockets
static WASI_SOCKETS_FNS: OnceLock<Vec<(&str, FuncParams, FuncResults)>> = OnceLock::new();

/// Retrieve or initialize the static list of functions related to sockets in WASI
pub fn get_wasi_sockets_fns() -> &'static Vec<(&'static str, FuncParams, FuncResults)> {
    WASI_SOCKETS_FNS.get_or_init(|| {
        Vec::from([
            (
                "wasi:sockets/network@0.2.0#drop-network",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/instance-network@0.2.0#instance-network",
                vec![],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/ip-name-lookup@0.2.0#resolve-addresses",
                vec![ValType::I32, ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/ip-name-lookup@0.2.0#resolve-next-address",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/ip-name-lookup@0.2.0#drop-resolve-address-stream",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/ip-name-lookup@0.2.0#subscribe",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp-create-socket@0.2.0#create-tcp-socket",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0#start-bind",
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
                "wasi:sockets/tcp@0.2.0#finish-bind",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0#start-connect",
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
                "wasi:sockets/tcp@0.2.0#finish-connect",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0#start-listen",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0#finish-listen",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0#accept",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0#local-address",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0#remote-address",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0#[method]tcp-socket.is-listening",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0#address-family",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0#set-listen-backlog-size",
                vec![ValType::I32, ValType::I64],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0#keep-alive-enabled",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0#set-keep-alive-enabled",
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0#keep-alive-idle-time",
                vec![ValType::I32, ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/tcp@0.2.0#set-keep-alive-idle-time",
                vec![ValType::I32, ValType::I64, ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/tcp@0.2.0#keep-alive-interval",
                vec![ValType::I32, ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/tcp@0.2.0#set-keep-alive-interval",
                vec![ValType::I32, ValType::I64, ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/tcp@0.2.0#keep-alive-count",
                vec![ValType::I32, ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/tcp@0.2.0#set-keep-alive-count",
                vec![ValType::I32, ValType::I32, ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/tcp@0.2.0#hop-limit",
                vec![ValType::I32, ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/tcp@0.2.0#set-hop-limit",
                vec![ValType::I32, ValType::I32, ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/tcp@0.2.0#receive-buffer-size",
                vec![ValType::I32, ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/tcp@0.2.0#set-receive-buffer-size",
                vec![ValType::I32, ValType::I64, ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/tcp@0.2.0#send-buffer-size",
                vec![ValType::I32, ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/tcp@0.2.0#set-send-buffer-size",
                vec![ValType::I32, ValType::I64, ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/tcp@0.2.0#subscribe",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0#shutdown",
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0#drop-tcp-socket",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/udp-create-socket@0.2.0#create-udp-socket",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0#start-bind",
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
                "wasi:sockets/udp@0.2.0#finish-bind",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0#local-address",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0#remote-address",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0#address-family",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0#unicast-hop-limit",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0#set-unicast-hop-limit",
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0#receive-buffer-size",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0#set-receive-buffer-size",
                vec![ValType::I32, ValType::I64],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0#send-buffer-size",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0#set-send-buffer-size",
                vec![ValType::I32, ValType::I64],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0#subscribe",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0#drop-udp-socket",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/udp@0.2.0#[method]udp-socket.stream",
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
                "wasi:sockets/udp@0.2.0#[method]incoming-datagram-stream.receive",
                vec![ValType::I32, ValType::I64, ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/udp@0.2.0#[method]incoming-datagram-stream.subscribe",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0#[resource-drop]incoming-datagram-stream",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/udp@0.2.0#[method]outgoing-datagram-stream.check-send",
                vec![ValType::I32, ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/udp@0.2.0#[method]outgoing-datagram-stream.send",
                vec![ValType::I32, ValType::I32, ValType::I32, ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/udp@0.2.0#[method]outgoing-datagram-stream.subscribe",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0#[resource-drop]outgoing-datagram-stream",
                vec![ValType::I32],
                vec![],
            ),
        ])
    })
}

/// Replace exports related to sockets in WASI to deny access
pub(crate) fn deny_sockets_virt(module: &mut Module) -> Result<()> {
    stub_virt(module, &["wasi:sockets/"], false)?;
    replace_or_insert_stub_for_exports(module, get_wasi_sockets_fns())
}
