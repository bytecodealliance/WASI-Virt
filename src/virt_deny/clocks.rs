use std::sync::OnceLock;

use anyhow::Result;
use walrus::{FuncParams, FuncResults, Module, ValType};

use crate::virt_io::stub_clocks_virt;

use super::replace_or_insert_stub_for_exports;

/// Functions that represent the environment functionality provided by WASI clocks
static WASI_CLOCK_FNS: OnceLock<Vec<(&str, FuncParams, FuncResults)>> = OnceLock::new();

/// Retrieve or initialize the static list of functions related to clocks in WASI
fn get_wasi_clock_fns() -> &'static Vec<(&'static str, FuncParams, FuncResults)> {
    WASI_CLOCK_FNS.get_or_init(|| {
        Vec::from([
            (
                "wasi:clocks/monotonic-clock#now",
                vec![],
                vec![ValType::I64],
            ),
            (
                "wasi:clocks/monotonic-clock#resolution",
                vec![],
                vec![ValType::I64],
            ),
            (
                "wasi:clocks/monotonic-clock#subscribe",
                vec![ValType::I64, ValType::I32],
                vec![ValType::I32],
            ),
            ("wasi:clocks/wall-clock#now", vec![], vec![ValType::I32]),
            (
                "wasi:clocks/wall-clock#resolution",
                vec![],
                vec![ValType::I32],
            ),
            (
                "wasi:clocks/wall-clock#subscribe",
                vec![ValType::I64, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:clocks/timezone#display",
                vec![ValType::I32, ValType::I64, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "cabi_post_wasi:clocks/timezone#display",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:clocks/timezone#utc-offset",
                vec![ValType::I32, ValType::I64, ValType::I32],
                vec![ValType::I32],
            ),
            (
                "wasi:clocks/timezone#drop-timezone",
                vec![ValType::I32],
                vec![],
            ),
        ])
    })
}

/// Replace exports related to clocks in WASI to deny access
pub(crate) fn deny_clocks_virt(module: &mut Module) -> Result<()> {
    stub_clocks_virt(module)?;
    replace_or_insert_stub_for_exports(module, get_wasi_clock_fns())
}
