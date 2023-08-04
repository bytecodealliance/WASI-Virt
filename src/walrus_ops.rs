use crate::mutator::{
    ir::Value, ActiveData, ActiveDataLocation, Data, DataKind, ExportItem, Function,
    FunctionBuilder, FunctionId, FunctionKind, GlobalKind, ImportKind, ImportedFunction, InitExpr,
    MemoryId, Module, ValType,
};
use anyhow::{bail, Context, Result};

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

pub(crate) fn get_memory_id(module: &Module) -> Result<MemoryId> {
    let mut mem_iter = module.memories.iter();
    let memory = mem_iter.next().context("Module does not export a memory")?;
    if mem_iter.next().is_some() {
        bail!("Multiple memories unsupported")
    }
    Ok(memory.id())
}

pub(crate) fn get_stack_global(module: &Module) -> Result<u32> {
    let stack_global_id = module
        .globals
        .iter()
        .find(|&global| global.name.as_deref() == Some("__stack_pointer"))
        .context("Unable to find __stack_pointer global name")?
        .id();
    let stack_global = module.globals.get(stack_global_id);
    let GlobalKind::Local(InitExpr::Value(Value::I32(stack_value))) = &stack_global.kind else {
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
    let GlobalKind::Local(InitExpr::Value(Value::I32(stack_value))) = &mut stack_global.kind else {
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

pub(crate) fn get_exported_func(module: &mut Module, name: &str) -> Result<FunctionId> {
    let exported_fn = module
        .exports
        .iter()
        .find(|expt| expt.name == name)
        .with_context(|| format!("Unable to find export '{name}'"))?;
    let ExportItem::Function(fid) = exported_fn.item else {
        bail!("{name} not a function");
    };
    Ok(fid)
}

pub(crate) fn add_stub_exported_func(
    module: &mut Module,
    export_name: &str,
    params: Vec<ValType>,
    results: Vec<ValType>,
) -> Result<()> {
    let exported_fn = module.exports.iter().find(|expt| expt.name == export_name);

    let mut builder = FunctionBuilder::new(&mut module.types, &params, &results);
    builder.func_body().unreachable();
    let local_func = builder.local_func(vec![]);
    let fid = module.funcs.add_local(local_func);

    // if it exists, replace it
    if let Some(exported_fn) = exported_fn {
        let export = module.exports.get_mut(exported_fn.id());
        export.item = ExportItem::Function(fid);
    } else {
        module.exports.add(export_name, ExportItem::Function(fid));
    }

    Ok(())
}

pub(crate) fn stub_imported_func(
    module: &mut Module,
    import_module: &str,
    import_name: &str,
    throw_if_not_found: bool,
) -> Result<()> {
    let imported_fn = match module
        .imports
        .iter()
        .find(|impt| impt.module == import_module && impt.name == import_name)
    {
        Some(found) => found,
        None => {
            if throw_if_not_found {
                bail!("Unable to find import {import_module}#{import_name} to stub");
            } else {
                return Ok(());
            }
        }
    };

    let ImportKind::Function(fid) = imported_fn.kind else {
        bail!("Unable to stub import {import_module}#{import_name}, as it is not an imported function");
    };
    let Function {
        kind: FunctionKind::Import(ImportedFunction { ty: tid, .. }),
        ..
    } = module.funcs.get(fid)
    else {
        bail!("Unable to stub import {import_module}#{import_name}, as it is not an imported function");
    };

    let ty = module.types.get(*tid);
    let (params, results) = (ty.params().to_vec(), ty.results().to_vec());

    let mut builder = FunctionBuilder::new(&mut module.types, &params, &results);
    builder.func_body().unreachable();
    let local_func = builder.local_func(vec![]);

    // substitute the local func into the imported func id
    let func = module.funcs.get_mut(fid);
    func.kind = FunctionKind::Local(local_func);

    // remove the import
    module.imports.delete(imported_fn.id());

    Ok(())
}

pub(crate) fn remove_exported_func(module: &mut Module, export_name: &str) -> Result<()> {
    let exported_fn = module
        .exports
        .iter()
        .find(|expt| expt.name == export_name)
        .with_context(|| format!("Unable to find export {export_name}"))?;

    module.exports.delete(exported_fn.id());

    Ok(())
}
