use std::sync::OnceLock;

use anyhow::Result;
use walrus::{FuncParams, FuncResults, Module, ValType};

use super::replace_or_insert_stub_for_exports;

/// Functions that represent the environment functionality provided by WASI randoms
static WASI_RANDOM_FNS: OnceLock<Vec<(&str, FuncParams, FuncResults)>> = OnceLock::new();

/// Retrieve or initialize the static list of functions related to randomness in WASI
fn get_wasi_random_fns() -> &'static Vec<(&'static str, FuncParams, FuncResults)> {
    WASI_RANDOM_FNS.get_or_init(|| {
        Vec::from([
            (
                "wasi:random/random@0.2.1#get-random-bytes",
                vec![ValType::I64],
                vec![ValType::I32],
            ),
            (
                "cabi_post_wasi:random/random@0.2.1#get-random-bytes",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:random/random@0.2.1#get-random-u64",
                vec![],
                vec![ValType::I64],
            ),
            (
                "wasi:random/insecure@0.2.1#get-insecure-random-bytes",
                vec![ValType::I64],
                vec![ValType::I32],
            ),
            (
                "cabi_post_wasi:random/insecure@0.2.1#get-insecure-random-bytes",
                vec![ValType::I32],
                vec![],
            ),
            (
                "wasi:random/insecure@0.2.1#get-insecure-random-u64",
                vec![],
                vec![ValType::I64],
            ),
            (
                "wasi:random/insecure-seed@0.2.1#insecure-seed",
                vec![],
                vec![ValType::I32],
            ),
        ])
    })
}

/// Replace exports related to randomness in WASI to deny access
pub(crate) fn deny_random_virt(module: &mut Module) -> Result<()> {
    replace_or_insert_stub_for_exports(module, get_wasi_random_fns())
}
