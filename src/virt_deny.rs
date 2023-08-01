use anyhow::Result;
use walrus::{Module, ValType};

use crate::walrus_ops::add_stub_exported_func;

// set exports to deny clock access
pub(crate) fn deny_clocks_virt(module: &mut Module) -> Result<()> {
    add_stub_exported_func(
        module,
        "wasi:clocks/monotonic-clock#now",
        vec![],
        vec![ValType::I64],
    )?;
    add_stub_exported_func(
        module,
        "wasi:clocks/monotonic-clock#resolution",
        vec![],
        vec![ValType::I64],
    )?;
    add_stub_exported_func(
        module,
        "wasi:clocks/monotonic-clock#subscribe",
        vec![ValType::I64, ValType::I32],
        vec![ValType::I32],
    )?;

    add_stub_exported_func(
        module,
        "wasi:clocks/wall-clock#now",
        vec![],
        vec![ValType::I32],
    )?;
    add_stub_exported_func(
        module,
        "wasi:clocks/wall-clock#resolution",
        vec![],
        vec![ValType::I32],
    )?;
    add_stub_exported_func(
        module,
        "wasi:clocks/wall-clock#subscribe",
        vec![ValType::I64, ValType::I32],
        vec![ValType::I32],
    )?;

    add_stub_exported_func(
        module,
        "wasi:clocks/timezone#display",
        vec![ValType::I32, ValType::I64, ValType::I32],
        vec![ValType::I32],
    )?;
    add_stub_exported_func(
        module,
        "cabi_post_wasi:clocks/timezone#display",
        vec![ValType::I32],
        vec![],
    )?;
    add_stub_exported_func(
        module,
        "wasi:clocks/timezone#utc-offset",
        vec![ValType::I32, ValType::I64, ValType::I32],
        vec![ValType::I32],
    )?;
    add_stub_exported_func(
        module,
        "wasi:clocks/timezone#drop-timezone",
        vec![ValType::I32],
        vec![],
    )?;

    Ok(())
}

pub(crate) fn deny_http_virt(module: &mut Module) -> Result<()> {
    add_stub_exported_func(
        module,
        "wasi:http/incoming-handler#handle",
        vec![ValType::I32, ValType::I32],
        vec![],
    )?;
    add_stub_exported_func(
        module,
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
    )?;

    // TODO: This needs completing

    Ok(())
}

pub(crate) fn deny_random_virt(module: &mut Module) -> Result<()> {
    add_stub_exported_func(
        module,
        "wasi:random/random#get-random-bytes",
        vec![ValType::I64],
        vec![ValType::I32],
    )?;
    add_stub_exported_func(
        module,
        "wasi:random/random#get-random-u64",
        vec![],
        vec![ValType::I64],
    )?;
    add_stub_exported_func(
        module,
        "wasi:random/insecure#get-insecure-random-bytes",
        vec![ValType::I64],
        vec![ValType::I32],
    )?;
    add_stub_exported_func(
        module,
        "wasi:random/insecure#get-insecure-random-u64",
        vec![],
        vec![ValType::I64],
    )?;
    add_stub_exported_func(
        module,
        "wasi:random/insecure-seed#insecure-seed",
        vec![ValType::I64],
        vec![ValType::I32],
    )?;
    Ok(())
}

pub(crate) fn deny_exit_virt(module: &mut Module) -> Result<()> {
    add_stub_exported_func(
        module,
        "wasi:cli-base/exit#exit",
        vec![ValType::I32],
        vec![],
    )?;
    Ok(())
}

pub(crate) fn deny_sockets_virt(module: &mut Module) -> Result<()> {
    todo!();
}
