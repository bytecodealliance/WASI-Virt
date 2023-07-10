use anyhow::{anyhow, Result};
use walrus::{
    ir::Value, ActiveData, ActiveDataLocation, Data, DataKind, Function, FunctionBuilder,
    FunctionKind, GlobalKind, ImportKind, ImportedFunction, InitExpr, MemoryId, Module,
};

pub(crate) fn get_active_data_segment<'a>(
    module: &'a mut Module,
    mem: MemoryId,
    addr: u32,
) -> Result<(&'a mut Data, usize)> {
    let data = match module.data.iter().find(|&data| {
        let DataKind::Active(active_data) = &data.kind else {
            return false;
        };
        if active_data.memory != mem {
            return false;
        };
        let ActiveDataLocation::Absolute(loc) = &active_data.location else {
            return false;
        };
        *loc <= addr && *loc + data.value.len() as u32 > addr
    }) {
        Some(data) => data,
        None => {
            return Err(anyhow!("Unable to find data section for env ptr"));
        }
    };
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

pub(crate) fn bump_stack_global<'a>(module: &'a mut Module, offset: i32) -> Result<u32> {
    let stack_global_id = match module.globals.iter().find(|&global| {
        if let Some(name) = &global.name {
            name == "__stack_pointer"
        } else {
            false
        }
    }) {
        Some(global) => global.id(),
        None => {
            return Err(anyhow!("Unable to find __stack_pointer global name"));
        }
    };
    let stack_global = module.globals.get_mut(stack_global_id);
    let GlobalKind::Local(InitExpr::Value(Value::I32(stack_value))) = &mut stack_global.kind else {
        return Err(anyhow!("Stack global is not a constant I32"));
    };
    if offset > *stack_value {
        return Err(anyhow!(
            "Stack size {} is smaller than the offset {offset}",
            *stack_value
        ));
    }
    let new_stack_value = *stack_value - offset;
    *stack_value = new_stack_value;
    Ok(new_stack_value as u32)
}

pub(crate) fn stub_imported_func<'a>(
    module: &'a mut Module,
    import_module: &str,
    import_name: &str,
) {
    let imported_fn = module
        .imports
        .iter()
        .find(|impt| impt.module == import_module && impt.name == import_name)
        .unwrap();

    let fid = match imported_fn.kind {
        ImportKind::Function(fid) => fid,
        _ => panic!(),
    };
    let tid = match module.funcs.get(fid) {
        Function {
            kind: FunctionKind::Import(ImportedFunction { ty, .. }),
            ..
        } => *ty,
        _ => panic!(),
    };

    let ty = module.types.get(tid);
    let (params, results) = (ty.params().to_vec(), ty.results().to_vec());

    let mut builder = FunctionBuilder::new(&mut module.types, &params, &results);
    builder.func_body().unreachable();
    let local_func = builder.local_func(vec![]);

    // substitute the local func into the imported func id
    let func = module.funcs.get_mut(fid);
    func.kind = FunctionKind::Local(local_func);

    // remove the import
    module.imports.delete(imported_fn.id());
}
