use std::sync::OnceLock;

use anyhow::Result;
use walrus::{FuncParams, FuncResults, Module, ValType};

use crate::virt_io::stub_sockets_virt;

use super::replace_or_insert_stub_for_exports;

/// Functions that represent the environment functionality provided by WASI sockets
static WASI_SOCKETS_FNS: OnceLock<Vec<(&str, FuncParams, FuncResults)>> = OnceLock::new();

/// Retrieve or initialize the static list of functions related to sockets in WASI
pub fn get_wasi_sockets_fns() -> &'static Vec<(&'static str, FuncParams, FuncResults)> {
    WASI_SOCKETS_FNS.get_or_init(|| {
        Vec::from([
            (
                "wasi:sockets/network@0.2.0-rc-2023-10-18#drop-network",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/instance-network@0.2.0-rc-2023-10-18#instance-network",
                vec![],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/ip-name-lookup@0.2.0-rc-2023-10-18#resolve-addresses",
                vec![
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
                "wasi:sockets/ip-name-lookup@0.2.0-rc-2023-10-18#resolve-next-address",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/ip-name-lookup@0.2.0-rc-2023-10-18#drop-resolve-address-stream",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/ip-name-lookup@0.2.0-rc-2023-10-18#subscribe",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp-create-socket@0.2.0-rc-2023-10-18#create-tcp-socket",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#start-bind",
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
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#finish-bind",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#start-connect",
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
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#finish-connect",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#start-listen",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#finish-listen",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#accept",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#local-address",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#remote-address",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#address-family",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#ipv6-only",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#set-ipv6-only",
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#set-listen-backlog-size",
                vec![ValType::I32, ValType::I64],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#keep-alive",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#set-keep-alive",
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#no-delay",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#set-no-delay",
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#unicast-hop-limit",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#set-unicast-hop-limit",
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#receive-buffer-size",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#set-receive-buffer-size",
                vec![ValType::I32, ValType::I64],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#send-buffer-size",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#set-send-buffer-size",
                vec![ValType::I32, ValType::I64],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#subscribe",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#shutdown",
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/tcp@0.2.0-rc-2023-10-18#drop-tcp-socket",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:sockets/udp-create-socket@0.2.0-rc-2023-10-18#create-udp-socket",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0-rc-2023-10-18#start-bind",
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
                "wasi:sockets/udp@0.2.0-rc-2023-10-18#finish-bind",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0-rc-2023-10-18#start-connect",
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
                "wasi:sockets/udp@0.2.0-rc-2023-10-18#finish-connect",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0-rc-2023-10-18#receive",
                vec![ValType::I32, ValType::I64],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0-rc-2023-10-18#send",
                vec![ValType::I32, ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0-rc-2023-10-18#local-address",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0-rc-2023-10-18#remote-address",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0-rc-2023-10-18#address-family",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0-rc-2023-10-18#ipv6-only",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0-rc-2023-10-18#set-ipv6-only",
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0-rc-2023-10-18#unicast-hop-limit",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0-rc-2023-10-18#set-unicast-hop-limit",
                vec![ValType::I32, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0-rc-2023-10-18#receive-buffer-size",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0-rc-2023-10-18#set-receive-buffer-size",
                vec![ValType::I32, ValType::I64],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0-rc-2023-10-18#send-buffer-size",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0-rc-2023-10-18#set-send-buffer-size",
                vec![ValType::I32, ValType::I64],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0-rc-2023-10-18#subscribe",
                vec![ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:sockets/udp@0.2.0-rc-2023-10-18#drop-udp-socket",
                vec![ValType::I32],
                vec![],
            ),
        ])
    })
}

/// Replace exports related to sockets in WASI to deny access
pub(crate) fn deny_sockets_virt(module: &mut Module) -> Result<()> {
    stub_sockets_virt(module)?;
    replace_or_insert_stub_for_exports(module, get_wasi_sockets_fns())
}
