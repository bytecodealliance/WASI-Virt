use std::sync::OnceLock;

use anyhow::Result;
use walrus::{FuncParams, FuncResults, Module, ValType};

use crate::walrus_ops::stub_virt;

use super::{replace_or_insert_stub_for_exports, StaticNsPkgIfaceTuple};

/// Functions that represent the environment functionality provided by WASI clocks
static WASI_CLOCK_FNS: OnceLock<Vec<(StaticNsPkgIfaceTuple, FuncParams, FuncResults)>> =
    OnceLock::new();

/// Retrieve or initialize the static list of functions related to clocks in WASI
fn get_wasi_clock_fns() -> &'static Vec<(StaticNsPkgIfaceTuple, FuncParams, FuncResults)> {
    WASI_CLOCK_FNS.get_or_init(|| {
        Vec::from([
            (
                ("wasi:clocks", "monotonic-clock", "now"),
                vec![],
                vec![ValType::I64],
            ),
            (
                ("wasi:clocks", "monotonic-clock", "resolution"),
                vec![],
                vec![ValType::I64],
            ),
            (
                ("wasi:clocks", "monotonic-clock", "subscribe-instant"),
                vec![ValType::I64],
                vec![ValType::I32],
            ),
            (
                ("wasi:clocks", "monotonic-clock", "subscribe-duration"),
                vec![ValType::I64],
                vec![ValType::I32],
            ),
            (
                ("wasi:clocks", "wall-clock", "now"),
                vec![],
                vec![ValType::I32],
            ),
            (
                ("wasi:clocks", "wall-clock", "resolution"),
                vec![],
                vec![ValType::I32],
            ),
        ])
    })
}

/// Replace exports related to clocks in WASI to deny access
pub(crate) fn deny_clocks_virt(module: &mut Module) -> Result<()> {
    stub_virt(module, &["wasi:clocks/"], false)?;
    replace_or_insert_stub_for_exports(module, get_wasi_clock_fns())
}
