use anyhow::{bail, Context, Result};
use walrus::{
    ir::Value, ActiveData, ActiveDataLocation, ConstExpr, Data, DataKind, GlobalKind, MemoryId,
    Module,
};

pub(crate) fn get_active_data_start(data: &Data, mem: MemoryId) -> Result<u32> {
    let DataKind::Active(active_data) = &data.kind else {
        bail!("Adapter data section is not active");
    };
    if active_data.memory != mem {
        bail!("Adapter data memory is not the expected memory id");
    }
    let ActiveDataLocation::Absolute(loc) = &active_data.location else {
        bail!("Adapter data memory is not absolutely offset");
    };
    Ok(*loc)
}

pub(crate) fn get_active_data_segment(
    module: &mut Module,
    mem: MemoryId,
    addr: u32,
) -> Result<(&mut Data, usize)> {
    let mut found_data: Option<&Data> = None;
    for data in module.data.iter() {
        let data_addr = get_active_data_start(data, mem)?;
        if data_addr <= addr {
            let best_match = match found_data {
                Some(found_data) => data_addr > get_active_data_start(found_data, mem)?,
                None => true,
            };
            if best_match {
                found_data = Some(data);
            }
        }
    }
    let data = found_data.context("Unable to find data section for ptr")?;
    let DataKind::Active(ActiveData {
        location: ActiveDataLocation::Absolute(loc),
        ..
    }) = &data.kind
    else {
        unreachable!()
    };
    let data_id = data.id();
    let offset = (addr - *loc) as usize;
    Ok((module.data.get_mut(data_id), offset))
}

pub(crate) fn get_stack_global(module: &Module) -> Result<u32> {
    let stack_global_id = module
        .globals
        .iter()
        .find(|&global| global.name.as_deref() == Some("__stack_pointer"))
        .context("Unable to find __stack_pointer global name")?
        .id();
    let stack_global = module.globals.get(stack_global_id);
    let GlobalKind::Local(ConstExpr::Value(Value::I32(stack_value))) = &stack_global.kind else {
        bail!("Stack global is not a constant I32");
    };
    Ok(*stack_value as u32)
}

pub(crate) fn bump_stack_global(module: &mut Module, offset: i32) -> Result<u32> {
    if offset % 8 != 0 {
        bail!("Stack global must be bumped by 8 byte alignment, offset of {offset} provided");
    }
    let stack_global_id = module
        .globals
        .iter()
        .find(|&global| global.name.as_deref() == Some("__stack_pointer"))
        .context("Unable to find __stack_pointer global name")?
        .id();
    let stack_global = module.globals.get_mut(stack_global_id);
    let GlobalKind::Local(ConstExpr::Value(Value::I32(stack_value))) = &mut stack_global.kind
    else {
        bail!("Stack global is not a constant I32");
    };
    if offset > *stack_value {
        bail!(
            "Stack size {} is smaller than the offset {offset}",
            *stack_value
        );
    }
    let new_stack_value = *stack_value - offset;
    *stack_value = new_stack_value;
    Ok(new_stack_value as u32)
}

pub(crate) fn strip_virt(module: &mut Module, subsystems: &[&str]) -> Result<()> {
    stub_virt(module, subsystems, true)?;
    let mut subsystem_exports = Vec::new();
    for export in module.exports.iter() {
        let export_name = if export.name.starts_with("cabi_post_") {
            &export.name[10..]
        } else {
            &export.name
        };
        if subsystems
            .iter()
            .any(|subsystem| export_name.starts_with(subsystem))
        {
            subsystem_exports.push(export.name.to_string());
        }
    }
    for export_name in subsystem_exports {
        module
            .exports
            .remove(&export_name)
            .with_context(|| format!("failed to strip function [{export_name}]"))?;
    }
    Ok(())
}

/// Replace imported WASI functions that implement subsystem access with no-ops
pub(crate) fn stub_virt(
    module: &mut Module,
    subsystems: &[&str],
    with_exports: bool,
) -> Result<()> {
    let mut subsystem_imports = Vec::new();
    for import in module.imports.iter() {
        let module_name = if with_exports && import.module.starts_with("[export]") {
            &import.module[8..]
        } else {
            &import.module
        };
        if subsystems
            .iter()
            .any(|subsystem| module_name.starts_with(subsystem))
        {
            subsystem_imports.push((import.module.to_string(), import.name.to_string()));
        }
    }
    for (module_name, func_name) in &subsystem_imports {
        if let Ok(fid) = module.imports.get_func(module_name, func_name) {
            module
                .replace_imported_func(fid, |(body, _)| {
                    body.unreachable();
                })
                .with_context(|| {
                    format!(
                        "failed to stub WASI functionality [{func_name}] in module [{module_name}]"
                    )
                })?;
        }
    }
    Ok(())
}
