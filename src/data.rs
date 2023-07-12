use std::collections::HashMap;

/// Data section
/// Because data is stack-allocated we create a corresponding byte vector as large
/// as the stack, zero fill it then populate it backwards from the
/// stack pointer. The final stack pointer will then be against the smaller stack.
/// This way, returned pointers are correct from the start, directly
/// corresponding to offsets in the slice, and can be known without having to
/// separately perform relocations.
/// We could alternatively do a smaller allocation then progressively grow,
/// while supporting reverse population, but this alloc seems fine for now.
pub(crate) struct DataSection {
    stack_ptr: usize,
    strings: HashMap<String, *const u8>,
    bytes: Vec<u8>,
}

impl DataSection {
    pub fn new(stack_start: usize) -> Self {
        let mut bytes = Vec::new();
        bytes.resize(stack_start, 0);
        DataSection {
            strings: HashMap::new(),
            stack_ptr: stack_start,
            bytes,
        }
    }
    /// Allocate some bytes into the data section, return the pointer
    pub fn bytes(&mut self, bytes: &[u8]) -> *const u8 {
        let end = self.stack_ptr;
        self.stack_ptr -= bytes.len();
        self.bytes[self.stack_ptr..end].copy_from_slice(bytes);
        self.stack_ptr as *const u8
    }
    /// Allocate a static string and return its pointer
    /// If the string already exists, return the existing pointer
    pub fn string(&mut self, str: &str) -> *const u8 {
        if let Some(&ptr) = self.strings.get(str) {
            return ptr;
        }
        // 1 for null termination
        // because of zero fill we are already null-terminated
        let end = self.stack_ptr - 1;
        self.stack_ptr -= str.as_bytes().len() + 1;
        self.bytes[self.stack_ptr as usize..end].copy_from_slice(&(str.len() as u32).to_le_bytes());
        self.strings
            .insert(str.to_string(), self.stack_ptr as *const u8);
        self.stack_ptr as *const u8
    }
    pub fn finish(&self) -> &[u8] {
        self.bytes.as_slice()
    }
}
