use crate::walrus_ops::{bump_stack_global, get_memory_id, get_realloc_func};
use anyhow::{bail, Result};
use std::collections::HashMap;
use walrus::{
    ir::BinaryOp, ActiveData, ActiveDataLocation, DataKind, FunctionBuilder, Module, ValType,
};

/// Data section
/// Because data is stack-allocated we create a corresponding byte vector as large
/// as the stack, zero fill it then populate it backwards from the
/// stack pointer. The final stack pointer will then be against the smaller stack.
/// This way, returned pointers are correct from the start, directly
/// corresponding to offsets in the slice, and can be known without having to
/// separately perform relocations.
/// We could alternatively do a smaller allocation then progressively grow,
/// while supporting reverse population, but this alloc seems fine for now.
pub(crate) struct Data {
    stack_start: usize,
    stack_ptr: usize,
    strings: HashMap<String, u32>,
    bytes: Vec<u8>,
    passive_segments: Vec<Vec<u8>>,
}

pub(crate) trait WasmEncode
where
    Self: Sized,
{
    fn align() -> usize;
    fn size() -> usize;
    fn encode(&self, bytes: &mut [u8]);
}

impl WasmEncode for u32 {
    fn align() -> usize {
        4
    }

    fn size() -> usize {
        4
    }

    fn encode(&self, bytes: &mut [u8]) {
        bytes[0..4].copy_from_slice(&self.to_le_bytes());
    }
}

impl Data {
    pub fn new(stack_start: usize) -> Self {
        let mut bytes = Vec::new();
        bytes.resize(stack_start, 0);
        Data {
            strings: HashMap::new(),
            stack_start,
            stack_ptr: stack_start,
            bytes,
            passive_segments: Vec::new(),
        }
    }

    pub fn passive_bytes(&mut self, bytes: &[u8]) -> u32 {
        let passive_idx = self.passive_segments.len();
        self.passive_segments.push(bytes.to_vec());
        passive_idx as u32
    }

    fn stack_alloc<'a>(&'a mut self, data_len: usize, align: usize) -> Result<&'a mut [u8]> {
        if data_len + align > self.stack_ptr {
            bail!("Out of stack space for file virtualization, use passive segments by decreasing the passive cutoff instead");
        }
        let mut new_stack_ptr = self.stack_ptr - data_len;
        if new_stack_ptr % align != 0 {
            let padding = align - (self.stack_ptr % align);
            new_stack_ptr -= padding;
        }
        self.stack_ptr = new_stack_ptr;
        Ok(&mut self.bytes[new_stack_ptr..new_stack_ptr + data_len])
    }

    pub fn stack_bytes(&mut self, bytes: &[u8]) -> Result<u32> {
        self.stack_alloc(bytes.len(), 1)?.copy_from_slice(bytes);
        Ok(self.stack_ptr as u32)
    }

    /// Allocate some bytes into the data section, return the pointer
    /// Note this is only safe for T being repr(C) / packed
    pub fn write_slice<T: WasmEncode>(&mut self, data: &[T]) -> Result<u32> {
        let size = T::size();
        let bytes = self.stack_alloc(data.len() * size, T::align())?;
        let mut cursor = 0;
        for item in data {
            item.encode(&mut bytes[cursor..cursor + size]);
            cursor += size;
        }
        Ok(self.stack_ptr as u32)
    }

    /// Allocate a static string and return its pointer
    /// If the string already exists, return the existing pointer
    pub fn string(&mut self, str: &str) -> Result<u32> {
        if let Some(&ptr) = self.strings.get(str) {
            return Ok(ptr);
        }
        // 1 for null termination
        // because of zero fill we are already null-terminated
        let len = str.as_bytes().len();
        let bytes = self.stack_alloc(len + 1, 1)?;
        bytes[0..len].copy_from_slice(str.as_bytes());
        bytes[len] = 0;
        self.strings.insert(str.to_string(), self.stack_ptr as u32);
        Ok(self.stack_ptr as u32)
    }
    pub fn finish(mut self, module: &mut Module) -> Result<()> {
        // stack embedding
        let memory = get_memory_id(module)?;
        let rem = (self.stack_start - self.stack_ptr) % 8;
        if rem != 0 {
            self.stack_ptr -= 8 - rem;
        }
        bump_stack_global(module, (self.stack_start - self.stack_ptr) as i32)?;
        module.data.add(
            DataKind::Active(ActiveData {
                memory,
                location: ActiveDataLocation::Absolute(self.stack_ptr as u32),
            }),
            self.bytes.as_slice()[self.stack_ptr..self.stack_start].to_vec(),
        );

        // passive segment embedding
        // we create one function for each passive segment, due to
        let alloc_fid = get_realloc_func(module)?;

        let offset_local = module.locals.add(ValType::I32);
        let len_local = module.locals.add(ValType::I32);
        for passive_segment in self.passive_segments {
            let passive_id = module.data.add(DataKind::Passive, passive_segment);

            // construct the passive segment allocation function
            let mut builder = FunctionBuilder::new(
                &mut module.types,
                &[ValType::I32, ValType::I32],
                &[ValType::I32],
            );
            builder
                .func_body()
                // cabi_realloc args
                .i32_const(0)
                .i32_const(0)
                .i32_const(4)
                // Last realloc arg is byte length to allocate
                .local_get(len_local)
                // mem init arg 0 - destination address
                .call(alloc_fid)
                // mem init arg 1 - source segment offset
                .local_get(offset_local)
                // mem init arg 2 - size of initialization
                .local_get(len_local)
                .memory_init(memory, passive_id);
            let local_func = builder.local_func(vec![offset_local, len_local]);

            // substitute the local func into the imported func id
            // let func = module.funcs.get_mut(fid);
            // func.kind = FunctionKind::Local(local_func);

            // Ok(())
        }

        // we then put all the passive functions into their own tables

        // finally we fill in the main orchestration function body using call_indirect

        Ok(())
    }
}
