use std::sync::OnceLock;

use anyhow::Result;
use semver::Version;
use walrus::{FuncParams, FuncResults, Module, ValType};

use super::replace_or_insert_stub_for_exports;
use crate::WITInterfaceNameParts;

/// Functions that represent the environment functionality provided by WASI exits
static WASI_EXIT_FNS: OnceLock<Vec<(WITInterfaceNameParts, FuncParams, FuncResults)>> =
    OnceLock::new();

/// Retrieve or initialize the static list of functions related to exiting in WASI
fn get_wasi_exit_fns() -> &'static Vec<(WITInterfaceNameParts, FuncParams, FuncResults)> {
    WASI_EXIT_FNS
        .get_or_init(|| Vec::from([(&("wasi", "cli", "exit", "exit"), vec![ValType::I32], vec![])]))
}

/// Replace exports related to exiting in WASI to deny access
///
/// # Arguments
///
/// * `module` - The module to deny
/// * `insert_wasi_version` - version of WASI to use when inserting stubs
///
pub(crate) fn deny_exit_virt(module: &mut Module, insert_wasi_version: &Version) -> Result<()> {
    replace_or_insert_stub_for_exports(module, get_wasi_exit_fns(), insert_wasi_version)
}
