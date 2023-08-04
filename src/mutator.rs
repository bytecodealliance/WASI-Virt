use std::{borrow::Cow, marker::PhantomData};

use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, CustomSection, DataSection, ElementSection, Elements,
    EntityType, ExportKind, ExportSection, FunctionSection, GlobalSection, GlobalType, HeapType,
    ImportSection, Instruction, MemArg, MemorySection, MemoryType, RefType, StartSection,
    TableSection, TableType, TagKind, TagSection, TagType, TypeSection,
};
use wasmparser::{Chunk, Parser, Payload};

use anyhow::{anyhow, Result};
use std::collections::{BTreeMap, HashMap};

/*
 * This file is a full local reimplementation of the Walrus API.
 * All API shapes are identical to the existing Walrus project.
 *
 * But instead of full reindexing, index mutations are stored, which
 * maintain orderings where possible, and avoids doing unnecessary
 * reindexing work.
 *
 * In theory this approach should therefore be faster than the current
 * Walrus architecture while having nicer index-retaining semantics.
 * If this code evolves far enough it could conceivably be used to
 * do a blanket upgrade of Walrus to this new architecture.
 *
 * If this approach is deemed too difficult to maintain, we can always
 * cut it, and rename all use of "mutator" to "walrus" since the APIs
 * are identical.
 *
 * But perhaps there's a future where the effort to improve the perf
 * and semantics works out.
 */

// ValType
#[derive(Debug, Clone)]
pub enum ValType {
    I32,
    I64,
    F32,
    F64,
    V128,
    Externref,
    Funcref,
}

impl From<&wasmparser::ValType> for ValType {
    fn from(ty: &wasmparser::ValType) -> ValType {
        match ty {
            wasmparser::ValType::I32 => ValType::I32,
            wasmparser::ValType::I64 => ValType::I64,
            wasmparser::ValType::F32 => ValType::F32,
            wasmparser::ValType::F64 => ValType::F64,
            wasmparser::ValType::V128 => ValType::V128,
            wasmparser::ValType::Ref(_) => panic!("Ref types unsupported"),
        }
    }
}

impl From<&ValType> for wasm_encoder::ValType {
    fn from(ty: &ValType) -> wasm_encoder::ValType {
        match ty {
            ValType::I32 => wasm_encoder::ValType::I32,
            ValType::I64 => wasm_encoder::ValType::I64,
            ValType::F32 => wasm_encoder::ValType::F32,
            ValType::F64 => wasm_encoder::ValType::F64,
            ValType::V128 => wasm_encoder::ValType::V128,
            ValType::Externref => todo!(),
            ValType::Funcref => todo!(),
        }
    }
}

// Module

pub struct Module<'a> {
    pub types: ModuleTypes,
    pub funcs: ModuleFunctions,
    pub imports: ModuleImports,
    pub exports: ModuleExports,
    pub globals: ModuleGlobals,
    // wasm binary
    binary: &'a [u8],
}

// Type

#[derive(Debug)]
pub struct Type {
    params: Vec<ValType>,
    results: Vec<ValType>,
}

impl Type {
    pub fn params(&self) -> &[ValType] {
        self.params.as_slice()
    }
    pub fn results(&self) -> &[ValType] {
        self.results.as_slice()
    }
}

type TypeId = Id<Type>;

pub struct ModuleTypes {
    mutations: IndexMutations<Type>,
}

impl ModuleTypes {
    fn new(originals: Vec<Type>) -> Self {
        ModuleTypes {
            mutations: IndexMutations::new(originals),
        }
    }

    pub fn delete(&mut self, id: TypeId) {
        self.mutations.delete(id);
    }

    pub fn add(&mut self, params: &[ValType], results: &[ValType]) -> TypeId {
        // assign the new function the first free index _after_ the import count
        // this allows filling in indices to try to match later funcs
        // eg in stubbing it can retain most function indexes (apart from imports)
        self.mutations.insert_at(
            Type {
                params: params.to_vec(),
                results: results.to_vec(),
            },
            0,
        )
    }

    pub fn get(&self, id: TypeId) -> &Type {
        self.mutations.get(id)
    }
}

// Global

#[derive(Debug)]
pub struct Global {
    /// This global's type.
    pub ty: ValType,
    /// Whether this global is mutable or not.
    pub mutable: bool,
    /// The kind of global this is
    pub kind: GlobalKind,
    /// The name of this data, used for debugging purposes in the `name`
    /// custom section.
    pub name: Option<String>,
}

#[derive(Debug)]
pub enum GlobalKind {
    Import(ImportId),
    Local(InitExpr),
}

#[derive(Debug)]
pub enum InitExpr {
    Value(Value),
    Global(GlobalId),
    RefNull(ValType),
    RefFunc(FunctionId),
}

#[derive(Debug)]
pub enum Value {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    V128(u128),
}

type GlobalId = Id<Global>;

pub struct ModuleGlobals {
    mutations: IndexMutations<Global>,
}

impl ModuleGlobals {
    fn new(originals: Vec<Global>) -> Self {
        ModuleGlobals {
            mutations: IndexMutations::new(originals),
        }
    }

    // pub fn add(&mut self, params: &[ValType], results: &[ValType]) -> GlobalId {
    //     // assign the new function the first free index _after_ the import count
    //     // this allows filling in indices to try to match later funcs
    //     // eg in stubbing it can retain most function indexes (apart from imports)
    //     self.mutations.insert_at(
    //         Type {
    //             params: params.to_vec(),
    //             results: results.to_vec(),
    //         },
    //         0,
    //     )
    // }

    pub fn get(&self, id: GlobalId) -> &Global {
        self.mutations.get(id)
    }

    pub fn get_mut(&self, id: GlobalId) -> &mut Global {
        self.mutations.get_mut(id)
    }

    pub fn delete(&mut self, id: GlobalId) {
        self.mutations.delete(id);
    }

    pub fn iter(&self) -> impl Iterator<Item = &Global> {
        self.mutations.iter()
    }
}

// Function

pub type FunctionId = Id<Function>;

#[derive(Debug)]
pub struct Function {
    pub kind: FunctionKind,
    pub name: Option<String>,
}

#[derive(Debug)]
pub enum FunctionKind {
    Import(ImportedFunction),
    Local(LocalFunction),
}

#[derive(Debug)]
pub struct ImportedFunction {
    pub import: ImportId,
    pub ty: TypeId,
}

#[derive(Debug)]
pub struct LocalFunction {
    pub ty: TypeId,
    pub fn_body: Box<[u8]>,
}

pub struct ModuleFunctions {
    mutations: IndexMutations<Function>,
    original_import_fn_len: u32,
}

impl ModuleFunctions {
    pub fn delete(&mut self, id: FunctionId) {
        self.mutations.delete(id);
    }

    pub fn add_local(&mut self, func: LocalFunction) -> FunctionId {
        // assign the new function the first free index _after_ the import count
        // this allows filling in indices to try to match later funcs
        // eg in stubbing it can retain most function indexes (apart from imports)
        self.mutations.insert_at(
            Function {
                kind: FunctionKind::Local(func),
                name: None,
            },
            self.original_import_fn_len,
        )
    }

    pub fn add_import(&mut self, ty: TypeId, import: ImportId) -> FunctionId {
        self.mutations.insert_at(
            Function {
                kind: FunctionKind::Import(ImportedFunction { import, ty }),
                name: None,
            },
            0,
        )
    }

    pub fn get(&self, id: FunctionId) -> &Function {
        self.mutations.get(id)
    }

    pub fn get_mut(&self, id: FunctionId) -> &mut Function {
        self.mutations.get_mut(id)
    }

    pub fn substitute(&mut self, id: FunctionId, substitution: FunctionId) {
        self.mutations.substitute(id, substitution)
    }
}

// Export

pub type ExportId = Id<Export>;

#[derive(Debug)]
pub struct Export {
    pub name: String,
    pub item: ExportItem,
    id: ExportId,
}

impl Export {
    pub fn id(&self) -> ExportId {
        self.id.clone()
    }
}

#[derive(Debug)]
pub enum ExportItem {
    Function(FunctionId),
    Global(GlobalId),
}

#[derive(Debug)]
pub struct ModuleExports {
    mutations: IndexMutations<Export>,
}

impl ModuleExports {
    fn new(originals: Vec<Export>) -> Self {
        ModuleExports {
            mutations: IndexMutations::new(originals),
        }
    }

    pub fn add(&mut self, name: &str, item: ExportItem) -> ExportId {
        let id = Id::InsertionIdx((self.mutations.insertions.len() as u32).into());
        self.mutations.insertions.push((
            0,
            Export {
                name: name.to_string(),
                item,
                id: id.clone(),
            },
        ));
        id
    }

    pub fn get(&self, id: ExportId) -> &Export {
        self.mutations.get(id)
    }

    pub fn get_mut(&self, id: ExportId) -> &mut Export {
        self.mutations.get_mut(id)
    }

    pub fn delete(&mut self, id: ExportId) {
        self.mutations.delete(id);
    }

    pub fn iter(&self) -> impl Iterator<Item = &Export> {
        self.mutations.iter()
    }
}

// Import

pub type ImportId = Id<Import>;

#[derive(Debug)]
pub struct Import {
    pub module: String,
    pub name: String,
    pub kind: ImportKind,
    id: ImportId,
}

impl Import {
    pub fn id(&self) -> ImportId {
        self.id.clone()
    }
}

#[derive(Debug)]
pub enum ImportKind {
    Function(FunctionId),
}

#[derive(Debug)]
pub struct ModuleImports {
    mutations: IndexMutations<Import>,
}

impl ModuleImports {
    fn new(originals: Vec<Import>) -> Self {
        ModuleImports {
            mutations: IndexMutations::new(originals),
        }
    }

    pub fn add(&mut self, module: String, name: String, kind: ImportKind) -> ImportId {
        let id = Id::InsertionIdx((self.mutations.insertions.len() as u32).into());
        self.mutations.insertions.push((
            0,
            Import {
                module,
                name,
                kind,
                id: id.clone(),
            },
        ));
        id
    }

    pub fn get(&self, id: ImportId) -> &Import {
        self.mutations.get(id)
    }

    pub fn delete(&mut self, id: ImportId) {
        self.mutations.delete(id);
    }

    pub fn iter(&self) -> impl Iterator<Item = &Import> {
        self.mutations.iter()
    }
}

// Generics

#[derive(Debug)]
pub enum Id<T> {
    OriginalIdx(Index<T>),
    InsertionIdx(Index<T>),
}

impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        match self {
            Id::OriginalIdx(original) => Id::OriginalIdx(Index::from(original.idx)),
            Id::InsertionIdx(insertion) => Id::InsertionIdx(Index::from(insertion.idx)),
        }
    }
}

impl<T> Copy for Id<T> {}

#[derive(Debug)]
pub struct Index<T> {
    idx: u32,
    phantom: PhantomData<T>,
}

impl<T> Copy for Index<T> {}

impl<T> Clone for Index<T> {
    fn clone(&self) -> Self {
        Index::from(self.idx)
    }
}

impl<T> From<u32> for Index<T> {
    fn from(idx: u32) -> Self {
        Index {
            idx,
            phantom: Default::default(),
        }
    }
}

impl<T> From<Index<T>> for u32 {
    fn from(id: Index<T>) -> u32 {
        id.idx
    }
}

#[derive(Debug)]
pub struct Mutation<T> {
    pub insertions: Vec<Index<T>>,
    pub deletion: bool,
}

impl<I> Mutation<I> {
    fn diff(&self) -> i32 {
        self.insertions.len() as i32 - if self.deletion { 1 } else { 0 }
    }
}

// monotonic index diffs
type Diffs<T> = BTreeMap<u32, Mutation<T>>;

#[derive(Debug)]
struct IndexMutations<T> {
    // replacements
    substitutions: HashMap<u32, Id<T>>,
    // originals
    originals: Vec<T>,
    // deleted
    deletions: Vec<Id<T>>,
    // inserted
    insertions: Vec<(u32, T)>,
}

impl<T> IndexMutations<T>
where
    T: std::fmt::Debug,
{
    fn new(originals: Vec<T>) -> Self {
        IndexMutations {
            substitutions: HashMap::new(),
            originals,
            deletions: Vec::new(),
            insertions: Vec::new(),
        }
    }

    fn delete(&mut self, id: Id<T>) {
        self.deletions.push(id);
    }

    fn insert_at(&mut self, insertion: T, original_idx: u32) -> Id<T> {
        self.insertions.push((original_idx, insertion));
        Id::InsertionIdx((self.insertions.len() as u32 - 1).into())
    }

    fn next_insertion_id(&self) -> Id<T> {
        Id::InsertionIdx((self.insertions.len() as u32).into())
    }

    fn assign_diffs(&mut self) -> Diffs<T> {
        let mut diffs: Diffs<T> = BTreeMap::new();

        // add deletion diffs
        for id in &self.deletions {
            match id {
                Id::OriginalIdx(original) => match diffs.get_mut(&original.idx) {
                    Some(cur_mutation) => {
                        cur_mutation.deletion = true;
                    }
                    None => {
                        diffs.insert(
                            original.idx,
                            Mutation {
                                insertions: vec![],
                                deletion: true,
                            },
                        );
                    }
                },
                Id::InsertionIdx(_) => {
                    todo!("Deleting an injection");
                }
            }
        }

        // insertions fill in over deletions
        // starting from the index that it supplied
        for (insertion_idx, (from_idx, _)) in self.insertions.iter().enumerate() {
            // add insertion diffs
            let mut cumulative_diff: i32 = 0;
            let mut new_idx = *from_idx;
            for (offset_original_idx, mutation) in &diffs {
                cumulative_diff += mutation.diff();
                if *offset_original_idx > new_idx {
                    new_idx = *offset_original_idx;
                    if cumulative_diff < 0 {
                        break;
                    }
                }
            }

            match diffs.get_mut(&from_idx) {
                Some(cur_mutation) => {
                    cur_mutation.insertions.push((insertion_idx as u32).into());
                }
                None => {
                    diffs.insert(
                        *from_idx,
                        Mutation {
                            insertions: vec![(insertion_idx as u32).into()],
                            deletion: false,
                        },
                    );
                }
            }
        }

        diffs
    }

    fn substitute(&mut self, id: Id<T>, substitution: Id<T>) {
        match id {
            Id::OriginalIdx(original) => {
                self.substitutions.insert(original.idx, substitution);
            }
            Id::InsertionIdx(_) => {
                todo!("Substituting an already inserted function not currently supported");
            }
        }
    }

    fn get(&self, id: Id<T>) -> &T {
        match id {
            Id::OriginalIdx(original) => &self.originals[original.idx as usize],
            Id::InsertionIdx(insertion) => &self.insertions[insertion.idx as usize].1,
        }
    }

    fn get_mut(&self, id: Id<T>) -> &mut T {
        match id {
            Id::OriginalIdx(original) => &mut self.originals[original.idx as usize],
            Id::InsertionIdx(insertion) => &mut self.insertions[insertion.idx as usize].1,
        }
    }

    fn get_idx(&self, id: Id<T>, diffs: &Diffs<T>) -> u32 {
        match id {
            Id::OriginalIdx(original) => {
                if let Some(sub) = self.substitutions.get(&original.idx) {
                    let res = self.get_idx(sub.clone(), diffs);
                    return res;
                }
                let mut new_idx = original.idx;
                for (offset_original_idx, mutation) in diffs {
                    if *offset_original_idx > original.idx {
                        break;
                    }
                    new_idx = new_idx.checked_add_signed(mutation.diff()).unwrap();
                }
                new_idx as u32
            }
            Id::InsertionIdx(insertion) => {
                let mut new_idx: i32 = 0;
                let mut last_offset_original_idx = 0;
                for (offset_original_idx, mutation) in diffs {
                    new_idx += *offset_original_idx as i32 - last_offset_original_idx as i32;

                    for (idx, cur_insertion) in mutation.insertions.iter().enumerate() {
                        if cur_insertion.idx == insertion.idx {
                            return (new_idx + idx as i32) as u32;
                        }
                    }

                    new_idx += mutation.insertions.len() as i32;

                    if !mutation.deletion {
                        new_idx += 1;
                    }

                    last_offset_original_idx = *offset_original_idx + 1;
                }
                unreachable!();
            }
        }
    }

    fn iter(&self) -> IndexMutationsIter<T> {
        IndexMutationsIter {
            mutations: &self,
            cur_idx: Id::OriginalIdx(0.into()),
        }
    }

    fn diff_iter<'a>(&'a self, diffs: &'a Diffs<T>) -> MutationsDiffIter<'a, T> {
        MutationsDiffIter {
            mutations: self,
            original_idx: 0,
            insertion_offset: 0,
            diffs,
        }
    }

    fn diff_iter_from<'a>(&'a self, diffs: &'a Diffs<T>, from: u32) -> MutationsDiffIter<'a, T> {
        MutationsDiffIter {
            mutations: self,
            original_idx: from,
            insertion_offset: 0,
            diffs,
        }
    }
}

pub struct IndexMutationsIter<'a, T> {
    mutations: &'a IndexMutations<T>,
    cur_idx: Id<T>,
}

impl<'a, T> Iterator for IndexMutationsIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        match self.cur_idx {
            Id::OriginalIdx(ref mut original) => {
                let out = self.mutations.originals.get(original.idx as usize);
                if out.is_none() {
                    if self.mutations.insertions.len() > 0 {
                        self.cur_idx = Id::InsertionIdx(0.into());
                        return self.next();
                    }
                } else {
                    original.idx += 1;
                }
                out
            }
            Id::InsertionIdx(insertion) => self
                .mutations
                .insertions
                .get(insertion.idx as usize)
                .map(|v| &v.1),
        }
    }
}

pub struct MutationsDiffIter<'a, T> {
    mutations: &'a IndexMutations<T>,
    original_idx: u32,
    insertion_offset: u32,
    diffs: &'a Diffs<T>,
}

#[derive(Debug)]
pub enum MutationIterStep<'a, T> {
    Original(&'a T),
    Skip,
    Insertion(&'a T),
}

impl<'a, T> Iterator for MutationsDiffIter<'a, T> {
    type Item = MutationIterStep<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(mutation) = self.diffs.get(&self.original_idx) {
            if self.insertion_offset < mutation.insertions.len() as u32 {
                let id = mutation.insertions[self.insertion_offset as usize];
                let result = Some(MutationIterStep::Insertion(
                    &self.mutations.insertions[id.idx as usize].1,
                ));
                self.insertion_offset += 1;
                return result;
            }
            self.insertion_offset = 0;
            if mutation.deletion {
                self.original_idx += 1;
                return Some(MutationIterStep::Skip);
            }
        }
        self.original_idx += 1;
        if self.original_idx <= self.mutations.originals.len() as u32 {
            Some(MutationIterStep::Original(
                &self.mutations.originals[(self.original_idx - 1) as usize],
            ))
        } else {
            None
        }
    }
}

impl<'a> Module<'a> {
    pub fn get_or_create_import_func(
        &mut self,
        module: String,
        name: String,
        ty: TypeId,
    ) -> (FunctionId, ImportId, usize) {
        let Some((idx, existing)) = self
            .imports
            .iter()
            .enumerate()
            .find(|&(_, impt)| impt.module == module && impt.name == name)
        else {
            let out = self.add_import_func(module, name, ty);
            return (out.0, out.1, 0);
        };
        let fid = match existing.kind {
            ImportKind::Function(fid) => fid,
        };
        (fid, existing.id, idx)
    }

    pub fn add_import_func(
        &mut self,
        module: String,
        name: String,
        ty: TypeId,
    ) -> (FunctionId, ImportId) {
        let fid = self.funcs.mutations.next_insertion_id();
        let import = self.imports.add(module, name, ImportKind::Function(fid));
        let func = self.funcs.add_import(ty, import);
        (func, import)
    }

    pub fn new(binary: &'a [u8]) -> Result<Self> {
        // do an initial parse of the import section to get the imported funcs
        let mut parser = Parser::new(0);
        let mut offset = 0;
        let mut types = Vec::new();
        let mut imports = Vec::new();
        let mut exports = Vec::new();
        let mut globals = Vec::new();
        let mut funcs = Vec::new();
        let mut original_import_fn_len = 0;
        loop {
            let payload = match parser.parse(&binary[offset..], true)? {
                Chunk::NeedMoreData(_) => unreachable!(),
                Chunk::Parsed { payload, consumed } => {
                    offset += consumed;
                    payload
                }
            };
            match payload {
                Payload::TypeSection(type_section_reader) => {
                    for core_type in type_section_reader {
                        match core_type?.structural_type {
                            wasmparser::StructuralType::Func(f) => types.push(Type {
                                params: f.params().iter().map(|v| v.into()).collect(),
                                results: f.results().iter().map(|v| v.into()).collect(),
                            }),
                            wasmparser::StructuralType::Array(_) => todo!(),
                            wasmparser::StructuralType::Struct(_) => todo!(),
                        }
                    }
                }
                Payload::ExportSection(expt_section_reader) => {
                    for export in expt_section_reader {
                        let wasmparser::Export { name, kind, index } = export?;
                        match kind {
                            wasmparser::ExternalKind::Func => {
                                let fid = Id::OriginalIdx(index.into());
                                exports.push(Export {
                                    name: name.to_string(),
                                    item: ExportItem::Function(fid),
                                    id: Id::OriginalIdx(original_import_fn_len.into()),
                                });
                            }
                            wasmparser::ExternalKind::Table => todo!("table exports"),
                            wasmparser::ExternalKind::Memory => todo!("memory exports"),
                            wasmparser::ExternalKind::Global => todo!("global exports"),
                            wasmparser::ExternalKind::Tag => todo!("tag exports"),
                        }
                    }
                }
                Payload::ImportSection(impt_section_reader) => {
                    for import in impt_section_reader {
                        let wasmparser::Import { ty, module, name } = import?;
                        match ty {
                            wasmparser::TypeRef::Func(tid) => {
                                let iid = Id::OriginalIdx(original_import_fn_len.into());
                                let fid = Id::OriginalIdx((funcs.len() as u32).into());
                                let tid = Id::OriginalIdx(tid.into());
                                funcs.push(Function {
                                    name: None,
                                    kind: FunctionKind::Import(ImportedFunction {
                                        import: iid.clone(),
                                        ty: tid,
                                    }),
                                });
                                imports.push(Import {
                                    module: module.to_string(),
                                    name: name.to_string(),
                                    kind: ImportKind::Function(fid),
                                    id: Id::OriginalIdx(original_import_fn_len.into()),
                                });
                            }
                            wasmparser::TypeRef::Table(_) => todo!("table imports"),
                            wasmparser::TypeRef::Memory(_) => todo!("memory imports"),
                            wasmparser::TypeRef::Global(_) => todo!("global imports"),
                            wasmparser::TypeRef::Tag(_) => todo!("tag imports"),
                        }
                        original_import_fn_len += 1;
                    }
                }
                Payload::FunctionSection(fn_section_reader) => {
                    for tidx in fn_section_reader {
                        funcs.push(Function {
                            name: None,
                            kind: FunctionKind::Local(LocalFunction {
                                ty: Id::OriginalIdx(tidx?.into()),
                                fn_body: Box::from(vec![]),
                            }),
                        });
                    }
                    break;
                }
                _ => {}
            }
        }

        Ok(Module {
            binary,
            types: ModuleTypes::new(types),
            funcs: ModuleFunctions {
                mutations: IndexMutations::new(funcs),
                original_import_fn_len,
            },
            imports: ModuleImports::new(imports),
            exports: ModuleExports::new(exports),
            globals: ModuleGlobals::new(globals),
        })
    }
    pub fn emit_wasm(&mut self) -> Result<Vec<u8>> {
        let mut module = wasm_encoder::Module::new();
        let mut type_section = TypeSection::new();
        let mut import_section = ImportSection::new();
        let mut func_section = FunctionSection::new();
        let mut table_section = TableSection::new();
        let mut memory_section = MemorySection::new();
        let mut tag_section = TagSection::new();
        let mut global_section = GlobalSection::new();
        let mut export_section = ExportSection::new();
        let mut start_section = None;
        let mut element_section = ElementSection::new();
        let mut code = CodeSection::new();
        let mut data_sections: Vec<DataSection> = Vec::new();
        let mut custom_sections: Vec<CustomSection> = Vec::new();
        let mut parser = Parser::new(0);
        let mut offset = 0;

        // new types appended at the end
        let type_diffs = self.types.mutations.assign_diffs();
        // imports assigned first-come-first-served
        let import_diffs = self.imports.mutations.assign_diffs();
        let export_diffs = self.exports.mutations.assign_diffs();
        let global_diffs = self.globals.mutations.assign_diffs();
        // functions must be assigned from the import length
        let func_diffs = self.funcs.mutations.assign_diffs();

        let mut code_iter = self
            .funcs
            .mutations
            .diff_iter_from(&func_diffs, self.funcs.original_import_fn_len);
        loop {
            let payload = match parser.parse(&self.binary[offset..], true)? {
                Chunk::NeedMoreData(_) => unreachable!(),
                Chunk::Parsed { payload, consumed } => {
                    offset += consumed;
                    payload
                }
            };

            match payload {
                Payload::Version { .. } => {}
                Payload::TypeSection(type_section_reader) => {
                    let mut iter = type_section_reader.into_iter();
                    for step in self.types.mutations.diff_iter(&type_diffs) {
                        let ty = match step {
                            MutationIterStep::Original(ty) => ty,
                            MutationIterStep::Skip => {
                                iter.next().unwrap()?;
                                continue;
                            }
                            MutationIterStep::Insertion(ty) => ty,
                        };
                        type_section.function(
                            ty.params.iter().map(|v| v.into()),
                            ty.results.iter().map(|v| v.into()),
                        );
                    }
                    assert!(iter.next().is_none());
                }
                Payload::ImportSection(impt_section_reader) => {
                    let mut iter = impt_section_reader.into_iter();
                    for step in self.imports.mutations.diff_iter(&import_diffs) {
                        let import = match step {
                            MutationIterStep::Original(import) => import,
                            MutationIterStep::Skip => {
                                iter.next().unwrap()?;
                                continue;
                            }
                            MutationIterStep::Insertion(import) => import,
                        };
                        let ty_id = match import.kind {
                            ImportKind::Function(fid) => {
                                match &self.funcs.mutations.get(fid).kind {
                                    FunctionKind::Import(import) => import.ty,
                                    FunctionKind::Local(local) => local.ty,
                                }
                            }
                        };
                        import_section.import(
                            &import.module,
                            &import.name,
                            EntityType::Function(self.types.mutations.get_idx(ty_id, &type_diffs)),
                        );
                    }
                    assert!(iter.next().is_none());
                }
                Payload::FunctionSection(fn_section_reader) => {
                    let mut iter = fn_section_reader.into_iter();
                    for step in self
                        .funcs
                        .mutations
                        .diff_iter_from(&func_diffs, self.funcs.original_import_fn_len)
                    {
                        let func = match step {
                            MutationIterStep::Original(func) => func,
                            MutationIterStep::Skip => {
                                iter.next().unwrap()?;
                                continue;
                            }
                            MutationIterStep::Insertion(func) => func,
                        };
                        let ty_id = match func {
                            Function {
                                kind: FunctionKind::Import(_),
                                ..
                            } => {
                                panic!("Unexpected import in function section");
                            }
                            Function {
                                kind: FunctionKind::Local(LocalFunction { ty: tid, .. }),
                                ..
                            } => tid.clone(),
                        };
                        func_section.function(self.types.mutations.get_idx(ty_id, &type_diffs));
                    }
                    assert!(iter.next().is_none());
                }
                Payload::TableSection(table_section_reader) => {
                    for table in table_section_reader {
                        let wasmparser::Table { ty, init: _ } = table?;
                        let wasmparser::TableType {
                            ref element_type,
                            initial,
                            maximum,
                        } = ty;
                        table_section.table(TableType {
                            element_type: RefType {
                                nullable: element_type.is_nullable(),
                                heap_type: heap_ty_map(&element_type.heap_type()),
                            },
                            minimum: initial,
                            maximum,
                        });
                    }
                }
                Payload::MemorySection(memory_section_reader) => {
                    for memory in memory_section_reader {
                        let wasmparser::MemoryType {
                            memory64,
                            shared,
                            initial,
                            maximum,
                        } = memory?;
                        memory_section.memory(MemoryType {
                            minimum: initial,
                            maximum,
                            memory64,
                            shared,
                        });
                    }
                }
                Payload::TagSection(tag_section_reader) => {
                    for tag in tag_section_reader {
                        let wasmparser::TagType {
                            kind,
                            func_type_idx,
                        } = tag?;
                        tag_section.tag(TagType {
                            kind: match kind {
                                wasmparser::TagKind::Exception => TagKind::Exception,
                            },
                            func_type_idx,
                        });
                    }
                }
                Payload::GlobalSection(global_section_reader) => {
                    for global in global_section_reader {
                        let wasmparser::Global {
                            ty:
                                wasmparser::GlobalType {
                                    content_type,
                                    mutable,
                                },
                            init_expr,
                        } = global?;
                        let init_expr_range = init_expr.get_binary_reader().range();
                        let init_expr_bytes =
                            &self.binary[init_expr_range.start..init_expr_range.end];
                        global_section.global(
                            GlobalType {
                                val_type: val_map(&content_type),
                                mutable,
                            },
                            &ConstExpr::raw(init_expr_bytes.to_vec()),
                        );
                    }
                }
                Payload::ExportSection(expt_section_reader) => {
                    let mut i = 0;
                    let mut iter = expt_section_reader.into_iter();
                    for step in self.exports.mutations.diff_iter(&export_diffs) {
                        let Export { name, item, id } = match step {
                            MutationIterStep::Original(export) => export,
                            MutationIterStep::Skip => {
                                iter.next().unwrap()?;
                                continue;
                            }
                            MutationIterStep::Insertion(export) => export,
                        };
                        export_section.export(
                            name,
                            match item {
                                ExportItem::Function(_) => ExportKind::Func,
                                ExportItem::Global(_) => ExportKind::Global,
                            },
                            match item {
                                ExportItem::Function(fid) => {
                                    self.funcs.mutations.get_idx(*fid, &func_diffs)
                                }
                                ExportItem::Global(gid) => {
                                    self.globals.mutations.get_idx(*gid, &global_diffs)
                                }
                            },
                        );
                    }
                    assert!(iter.next().is_none());
                }
                Payload::StartSection { func, .. } => {
                    start_section = Some(StartSection {
                        function_index: self
                            .funcs
                            .mutations
                            .get_idx(Id::OriginalIdx(func.into()), &func_diffs),
                    });
                }
                Payload::ElementSection(el_section_reader) => {
                    for element in el_section_reader {
                        let mut element_entries = Vec::new();
                        let wasmparser::Element { kind, items, .. } = element?;
                        match items {
                            wasmparser::ElementItems::Functions(fns) => {
                                for func in fns {
                                    let fidx = func?;
                                    let idx = self
                                        .funcs
                                        .mutations
                                        .get_idx(Id::OriginalIdx(fidx.into()), &func_diffs);
                                    element_entries.push(idx);
                                }
                            }
                            wasmparser::ElementItems::Expressions(..) => {
                                return Err(anyhow!("Expression elements not supported"));
                            }
                        }
                        match kind {
                            wasmparser::ElementKind::Active {
                                offset_expr,
                                table_index,
                            } => {
                                if let Some(table_index) = table_index {
                                    if table_index > 0 {
                                        todo!("multiple tables");
                                    }
                                }
                                let mut multiple = false;
                                for op in offset_expr.get_operators_reader() {
                                    match op? {
                                        wasmparser::Operator::I32Const { value } => {
                                            if value != 1 {
                                                return Err(anyhow!(
                                                    "Unexpected table start offset"
                                                ));
                                            }
                                        }
                                        wasmparser::Operator::End => break,
                                        _ => return Err(anyhow!("Unexpected const expr")),
                                    };
                                    if multiple {
                                        return Err(anyhow!(
                                            "Unexpected multiple ops in constant expression"
                                        ));
                                    }
                                    multiple = true;
                                }
                                element_section.active(
                                    None,
                                    &ConstExpr::i32_const(1),
                                    RefType::FUNCREF,
                                    Elements::Functions(&element_entries[..]),
                                );
                            }
                            wasmparser::ElementKind::Passive
                            | wasmparser::ElementKind::Declared => {
                                return Err(anyhow!(
                                    "Passive and declared table elements not yet supported"
                                ));
                            }
                        }
                    }
                }
                Payload::CodeSectionStart { .. } => {}
                Payload::CodeSectionEntry(parse_func) => {
                    let mut step = code_iter.next().unwrap();
                    while let MutationIterStep::Insertion(insertion) = step {
                        match insertion {
                            Function {
                                kind: FunctionKind::Import(_),
                                ..
                            } => {
                                // ignore imports
                            }
                            Function {
                                kind: FunctionKind::Local(LocalFunction { ref fn_body, .. }),
                                ..
                            } => {
                                code.raw(fn_body);
                            }
                        }
                        step = code_iter.next().unwrap();
                    }

                    if matches!(step, MutationIterStep::Skip) {
                        continue;
                    }

                    // TODO: Mutation handling...
                    let mut locals: Vec<(u32, wasm_encoder::ValType)> = Vec::new();
                    for local in parse_func.get_locals_reader()?.into_iter() {
                        let (cnt, val_type) = local?;
                        locals.push((cnt, val_map(&val_type)));
                    }

                    let mut func = wasm_encoder::Function::new(locals);
                    for op in parse_func.get_operators_reader()? {
                        let instruction = op_map(&op?);
                        func.instruction(&match instruction {
                            // Call -> fn offset
                            Instruction::Call(fidx) => {
                                let new_idx = self
                                    .funcs
                                    .mutations
                                    .get_idx(Id::OriginalIdx(fidx.into()), &func_diffs);
                                Instruction::Call(new_idx)
                            }
                            Instruction::CallIndirect { ty, table } => {
                                let ty = self
                                    .types
                                    .mutations
                                    .get_idx(Id::OriginalIdx(ty.into()), &type_diffs);
                                Instruction::CallIndirect { ty, table }
                            }
                            _ => instruction,
                        });
                    }
                    code.function(&func);
                }
                Payload::DataSection(data_section_reader) => {
                    let mut section = DataSection::new();
                    for item in data_section_reader.into_iter_with_offsets() {
                        let (_, data) = item?;
                        section.raw(&self.binary[data.range.start..data.range.end]);
                    }
                    data_sections.push(section);
                }
                Payload::CustomSection(custom_section_reader) => {
                    let name = custom_section_reader.name();
                    let data = custom_section_reader.data();
                    let section = CustomSection {
                        name: name.into(),
                        data: data.into(),
                    };
                    custom_sections.push(section);
                }
                Payload::DataCountSection { .. } => {}
                Payload::UnknownSection { .. } => return Err(anyhow!("Unknown section")),
                Payload::ComponentSection { .. }
                | Payload::ComponentInstanceSection(_)
                | Payload::ComponentAliasSection(_)
                | Payload::ComponentTypeSection(_)
                | Payload::ComponentCanonicalSection(_)
                | Payload::ComponentStartSection { .. }
                | Payload::ComponentImportSection(_)
                | Payload::ComponentExportSection(_)
                | Payload::CoreTypeSection(_)
                | Payload::ModuleSection { .. }
                | Payload::InstanceSection(_) => {
                    return Err(anyhow!("Unexpected component section"))
                }
                Payload::End(_) => break,
            }
        }

        // post-insertions for code iter
        while let Some(_) = code_iter.next() {
            todo!("Code appends");
        }

        module.section(&type_section);
        module.section(&import_section);
        module.section(&func_section);
        module.section(&table_section);
        module.section(&memory_section);
        if tag_section.len() > 0 {
            module.section(&tag_section);
        }
        module.section(&global_section);
        module.section(&export_section);
        if start_section.is_some() {
            module.section(&start_section.unwrap());
        }
        module.section(&element_section);
        module.section(&code);
        for ref data in data_sections {
            module.section(data);
        }
        for ref custom in custom_sections {
            module.section(custom);
        }

        Ok(module.finish())
    }
}

fn heap_ty_map(heap_type: &wasmparser::HeapType) -> HeapType {
    match heap_type {
        wasmparser::HeapType::Func => HeapType::Func,
        wasmparser::HeapType::Extern => HeapType::Extern,
        wasmparser::HeapType::Any => HeapType::Any,
        wasmparser::HeapType::None => HeapType::None,
        wasmparser::HeapType::NoExtern => HeapType::NoExtern,
        wasmparser::HeapType::NoFunc => HeapType::NoFunc,
        wasmparser::HeapType::Eq => HeapType::Eq,
        wasmparser::HeapType::Struct => HeapType::Struct,
        wasmparser::HeapType::Array => HeapType::Array,
        wasmparser::HeapType::I31 => HeapType::I31,
        wasmparser::HeapType::Indexed(idx) => HeapType::Indexed(*idx),
    }
}

fn val_map(ty: &wasmparser::ValType) -> wasm_encoder::ValType {
    match ty {
        wasmparser::ValType::I32 => wasm_encoder::ValType::I32,
        wasmparser::ValType::I64 => wasm_encoder::ValType::I64,
        wasmparser::ValType::F32 => wasm_encoder::ValType::F32,
        wasmparser::ValType::F64 => wasm_encoder::ValType::F64,
        wasmparser::ValType::V128 => wasm_encoder::ValType::V128,
        wasmparser::ValType::Ref(ty) => wasm_encoder::ValType::Ref(ref_map(ty)),
    }
}

fn ref_map(ty: &wasmparser::RefType) -> RefType {
    let nullable = ty.is_nullable();
    let heap_type = &ty.heap_type();
    RefType {
        nullable,
        heap_type: heap_ty_map(heap_type),
    }
}

fn memarg_map(memarg: &wasmparser::MemArg) -> MemArg {
    let wasmparser::MemArg {
        align,
        offset,
        memory,
        ..
    } = memarg;
    MemArg {
        align: *align as u32,
        offset: *offset,
        memory_index: *memory,
    }
}

fn blockty_map(blockty: &wasmparser::BlockType) -> BlockType {
    match blockty {
        wasmparser::BlockType::Empty => BlockType::Empty,
        wasmparser::BlockType::Type(ty) => BlockType::Result(val_map(ty)),
        wasmparser::BlockType::FuncType(ty) => BlockType::FunctionType(*ty),
    }
}

fn op_map<'a>(op: &wasmparser::Operator) -> Instruction<'a> {
    match op {
        wasmparser::Operator::CallRef { .. } => todo!("call ref"),
        wasmparser::Operator::ReturnCallRef { .. } => todo!("return call ref"),
        wasmparser::Operator::RefAsNonNull { .. } => todo!("ref as non null"),
        wasmparser::Operator::BrOnNonNull { .. } => todo!("br on non null"),
        wasmparser::Operator::BrOnNull { .. } => todo!("br on null"),
        wasmparser::Operator::Unreachable => Instruction::Unreachable,
        wasmparser::Operator::Nop => Instruction::Nop,
        wasmparser::Operator::Block { blockty } => Instruction::Block(blockty_map(blockty)),
        wasmparser::Operator::Loop { blockty } => Instruction::Loop(blockty_map(blockty)),
        wasmparser::Operator::If { blockty } => Instruction::If(blockty_map(blockty)),
        wasmparser::Operator::Else => Instruction::Else,
        wasmparser::Operator::Try { blockty } => Instruction::Try(blockty_map(blockty)),
        wasmparser::Operator::Catch { .. } => todo!("catch"),
        wasmparser::Operator::Throw { .. } => todo!("throw"),
        wasmparser::Operator::Rethrow { .. } => todo!("rethrow"),
        wasmparser::Operator::End => Instruction::End,
        wasmparser::Operator::Br { relative_depth } => Instruction::Br(*relative_depth),
        wasmparser::Operator::BrIf { relative_depth } => Instruction::BrIf(*relative_depth),
        wasmparser::Operator::BrTable { targets } => {
            let mut out_targets = Vec::new();
            for target in targets.targets() {
                out_targets.push(target.unwrap());
            }
            Instruction::BrTable(Cow::from(out_targets), targets.default())
        }
        wasmparser::Operator::Return => Instruction::Return,
        wasmparser::Operator::Call { function_index } => Instruction::Call(*function_index),
        wasmparser::Operator::CallIndirect {
            type_index,
            table_index,
            ..
        } => Instruction::CallIndirect {
            ty: *type_index,
            table: *table_index,
        },
        wasmparser::Operator::ReturnCall { .. } => todo!("returncall"),
        wasmparser::Operator::ReturnCallIndirect { .. } => todo!("returncallindirect"),
        wasmparser::Operator::Delegate { .. } => todo!("delegate"),
        wasmparser::Operator::CatchAll => todo!("catchall"),
        wasmparser::Operator::Drop => Instruction::Drop,
        wasmparser::Operator::Select => Instruction::Select,
        wasmparser::Operator::TypedSelect { .. } => todo!("typedselect"),
        wasmparser::Operator::LocalGet { local_index } => Instruction::LocalGet(*local_index),
        wasmparser::Operator::LocalSet { local_index } => Instruction::LocalSet(*local_index),
        wasmparser::Operator::LocalTee { local_index } => Instruction::LocalTee(*local_index),
        wasmparser::Operator::GlobalGet { global_index } => Instruction::GlobalGet(*global_index),
        wasmparser::Operator::GlobalSet { global_index } => Instruction::GlobalSet(*global_index),
        wasmparser::Operator::I32Load { memarg } => Instruction::I32Load(memarg_map(memarg)),
        wasmparser::Operator::I64Load { memarg } => Instruction::I64Load(memarg_map(memarg)),
        wasmparser::Operator::F32Load { memarg } => Instruction::F32Load(memarg_map(memarg)),
        wasmparser::Operator::F64Load { memarg } => Instruction::F64Load(memarg_map(memarg)),
        wasmparser::Operator::I32Load8S { memarg } => Instruction::I32Load8S(memarg_map(memarg)),
        wasmparser::Operator::I32Load8U { memarg } => Instruction::I32Load8U(memarg_map(memarg)),
        wasmparser::Operator::I32Load16S { memarg } => Instruction::I32Load16S(memarg_map(memarg)),
        wasmparser::Operator::I32Load16U { memarg } => Instruction::I32Load16U(memarg_map(memarg)),
        wasmparser::Operator::I64Load8S { memarg } => Instruction::I64Load8S(memarg_map(memarg)),
        wasmparser::Operator::I64Load8U { memarg } => Instruction::I64Load8U(memarg_map(memarg)),
        wasmparser::Operator::I64Load16S { memarg } => Instruction::I64Load16S(memarg_map(memarg)),
        wasmparser::Operator::I64Load16U { memarg } => Instruction::I64Load16U(memarg_map(memarg)),
        wasmparser::Operator::I64Load32S { memarg } => Instruction::I64Load32S(memarg_map(memarg)),
        wasmparser::Operator::I64Load32U { memarg } => Instruction::I64Load32U(memarg_map(memarg)),
        wasmparser::Operator::I32Store { memarg } => Instruction::I32Store(memarg_map(memarg)),
        wasmparser::Operator::I64Store { memarg } => Instruction::I64Store(memarg_map(memarg)),
        wasmparser::Operator::F32Store { memarg } => Instruction::F32Store(memarg_map(memarg)),
        wasmparser::Operator::F64Store { memarg } => Instruction::F64Store(memarg_map(memarg)),
        wasmparser::Operator::I32Store8 { memarg } => Instruction::I32Store8(memarg_map(memarg)),
        wasmparser::Operator::I32Store16 { memarg } => Instruction::I32Store16(memarg_map(memarg)),
        wasmparser::Operator::I64Store8 { memarg } => Instruction::I64Store8(memarg_map(memarg)),
        wasmparser::Operator::I64Store16 { memarg } => Instruction::I64Store16(memarg_map(memarg)),
        wasmparser::Operator::I64Store32 { memarg } => Instruction::I64Store32(memarg_map(memarg)),
        wasmparser::Operator::MemorySize { mem: _, mem_byte } => {
            Instruction::MemorySize(*mem_byte as u32)
        }
        wasmparser::Operator::MemoryGrow { mem_byte, .. } => {
            Instruction::MemoryGrow(*mem_byte as u32)
        }
        wasmparser::Operator::I32Const { value } => Instruction::I32Const(*value),
        wasmparser::Operator::I64Const { value } => Instruction::I64Const(*value),
        wasmparser::Operator::F32Const { value } => {
            Instruction::F32Const(f32::from_bits(value.bits()))
        }
        wasmparser::Operator::F64Const { value } => {
            Instruction::F64Const(f64::from_bits(value.bits()))
        }
        wasmparser::Operator::RefNull { .. } => todo!("refnull"),
        wasmparser::Operator::RefIsNull => Instruction::RefIsNull,
        wasmparser::Operator::RefFunc { .. } => todo!("reffunc"),
        wasmparser::Operator::I32Eqz => Instruction::I32Eqz,
        wasmparser::Operator::I32Eq => Instruction::I32Eq,
        wasmparser::Operator::I32Ne => Instruction::I32Ne,
        wasmparser::Operator::I32LtS => Instruction::I32LtS,
        wasmparser::Operator::I32LtU => Instruction::I32LtU,
        wasmparser::Operator::I32GtS => Instruction::I32GtS,
        wasmparser::Operator::I32GtU => Instruction::I32GtU,
        wasmparser::Operator::I32LeS => Instruction::I32LeS,
        wasmparser::Operator::I32LeU => Instruction::I32LeU,
        wasmparser::Operator::I32GeS => Instruction::I32GeS,
        wasmparser::Operator::I32GeU => Instruction::I32GeU,
        wasmparser::Operator::I64Eqz => Instruction::I64Eqz,
        wasmparser::Operator::I64Eq => Instruction::I64Eq,
        wasmparser::Operator::I64Ne => Instruction::I64Ne,
        wasmparser::Operator::I64LtS => Instruction::I64LtS,
        wasmparser::Operator::I64LtU => Instruction::I64LtU,
        wasmparser::Operator::I64GtS => Instruction::I64GtS,
        wasmparser::Operator::I64GtU => Instruction::I64GtU,
        wasmparser::Operator::I64LeS => Instruction::I64LeS,
        wasmparser::Operator::I64LeU => Instruction::I64LeU,
        wasmparser::Operator::I64GeS => Instruction::I64GeS,
        wasmparser::Operator::I64GeU => Instruction::I64GeU,
        wasmparser::Operator::F32Eq => Instruction::F32Eq,
        wasmparser::Operator::F32Ne => Instruction::F32Ne,
        wasmparser::Operator::F32Lt => Instruction::F32Lt,
        wasmparser::Operator::F32Gt => Instruction::F32Gt,
        wasmparser::Operator::F32Le => Instruction::F32Le,
        wasmparser::Operator::F32Ge => Instruction::F32Ge,
        wasmparser::Operator::F64Eq => Instruction::F64Eq,
        wasmparser::Operator::F64Ne => Instruction::F64Ne,
        wasmparser::Operator::F64Lt => Instruction::F64Lt,
        wasmparser::Operator::F64Gt => Instruction::F64Gt,
        wasmparser::Operator::F64Le => Instruction::F64Le,
        wasmparser::Operator::F64Ge => Instruction::F64Ge,
        wasmparser::Operator::I32Clz => Instruction::I32Clz,
        wasmparser::Operator::I32Ctz => Instruction::I32Ctz,
        wasmparser::Operator::I32Popcnt => Instruction::I32Popcnt,
        wasmparser::Operator::I32Add => Instruction::I32Add,
        wasmparser::Operator::I32Sub => Instruction::I32Sub,
        wasmparser::Operator::I32Mul => Instruction::I32Mul,
        wasmparser::Operator::I32DivS => Instruction::I32DivS,
        wasmparser::Operator::I32DivU => Instruction::I32DivU,
        wasmparser::Operator::I32RemS => Instruction::I32RemS,
        wasmparser::Operator::I32RemU => Instruction::I32RemU,
        wasmparser::Operator::I32And => Instruction::I32And,
        wasmparser::Operator::I32Or => Instruction::I32Or,
        wasmparser::Operator::I32Xor => Instruction::I32Xor,
        wasmparser::Operator::I32Shl => Instruction::I32Shl,
        wasmparser::Operator::I32ShrS => Instruction::I32ShrS,
        wasmparser::Operator::I32ShrU => Instruction::I32ShrU,
        wasmparser::Operator::I32Rotl => Instruction::I32Rotl,
        wasmparser::Operator::I32Rotr => Instruction::I32Rotr,
        wasmparser::Operator::I64Clz => Instruction::I64Clz,
        wasmparser::Operator::I64Ctz => Instruction::I64Ctz,
        wasmparser::Operator::I64Popcnt => Instruction::I64Popcnt,
        wasmparser::Operator::I64Add => Instruction::I64Add,
        wasmparser::Operator::I64Sub => Instruction::I64Sub,
        wasmparser::Operator::I64Mul => Instruction::I64Mul,
        wasmparser::Operator::I64DivS => Instruction::I64DivS,
        wasmparser::Operator::I64DivU => Instruction::I64DivU,
        wasmparser::Operator::I64RemS => Instruction::I64RemS,
        wasmparser::Operator::I64RemU => Instruction::I64RemU,
        wasmparser::Operator::I64And => Instruction::I64And,
        wasmparser::Operator::I64Or => Instruction::I64Or,
        wasmparser::Operator::I64Xor => Instruction::I64Xor,
        wasmparser::Operator::I64Shl => Instruction::I64Shl,
        wasmparser::Operator::I64ShrS => Instruction::I64ShrS,
        wasmparser::Operator::I64ShrU => Instruction::I64ShrU,
        wasmparser::Operator::I64Rotl => Instruction::I64Rotl,
        wasmparser::Operator::I64Rotr => Instruction::I64Rotr,
        wasmparser::Operator::F32Abs => Instruction::F32Abs,
        wasmparser::Operator::F32Neg => Instruction::F32Neg,
        wasmparser::Operator::F32Ceil => Instruction::F32Ceil,
        wasmparser::Operator::F32Floor => Instruction::F32Floor,
        wasmparser::Operator::F32Trunc => Instruction::F32Trunc,
        wasmparser::Operator::F32Nearest => Instruction::F32Nearest,
        wasmparser::Operator::F32Sqrt => Instruction::F32Sqrt,
        wasmparser::Operator::F32Add => Instruction::F32Add,
        wasmparser::Operator::F32Sub => Instruction::F32Sub,
        wasmparser::Operator::F32Mul => Instruction::F32Mul,
        wasmparser::Operator::F32Div => Instruction::F32Div,
        wasmparser::Operator::F32Min => Instruction::F32Min,
        wasmparser::Operator::F32Max => Instruction::F32Max,
        wasmparser::Operator::F32Copysign => Instruction::F32Copysign,
        wasmparser::Operator::F64Abs => Instruction::F64Abs,
        wasmparser::Operator::F64Neg => Instruction::F64Neg,
        wasmparser::Operator::F64Ceil => Instruction::F64Ceil,
        wasmparser::Operator::F64Floor => Instruction::F64Floor,
        wasmparser::Operator::F64Trunc => Instruction::F64Trunc,
        wasmparser::Operator::F64Nearest => Instruction::F64Nearest,
        wasmparser::Operator::F64Sqrt => Instruction::F64Sqrt,
        wasmparser::Operator::F64Add => Instruction::F64Add,
        wasmparser::Operator::F64Sub => Instruction::F64Sub,
        wasmparser::Operator::F64Mul => Instruction::F64Mul,
        wasmparser::Operator::F64Div => Instruction::F64Div,
        wasmparser::Operator::F64Min => Instruction::F64Min,
        wasmparser::Operator::F64Max => Instruction::F64Max,
        wasmparser::Operator::F64Copysign => Instruction::F64Copysign,
        wasmparser::Operator::I32WrapI64 => Instruction::I32WrapI64,
        wasmparser::Operator::I32TruncF32S => Instruction::I32TruncF32S,
        wasmparser::Operator::I32TruncF32U => Instruction::I32TruncF32U,
        wasmparser::Operator::I32TruncF64S => Instruction::I32TruncF64S,
        wasmparser::Operator::I32TruncF64U => Instruction::I32TruncF64U,
        wasmparser::Operator::I64ExtendI32S => Instruction::I64ExtendI32S,
        wasmparser::Operator::I64ExtendI32U => Instruction::I64ExtendI32U,
        wasmparser::Operator::I64TruncF32S => Instruction::I64TruncF32S,
        wasmparser::Operator::I64TruncF32U => Instruction::I64TruncF32U,
        wasmparser::Operator::I64TruncF64S => Instruction::I64TruncF64S,
        wasmparser::Operator::I64TruncF64U => Instruction::I64TruncF64U,
        wasmparser::Operator::F32ConvertI32S => Instruction::F32ConvertI32S,
        wasmparser::Operator::F32ConvertI32U => Instruction::F32ConvertI32U,
        wasmparser::Operator::F32ConvertI64S => Instruction::F32ConvertI64S,
        wasmparser::Operator::F32ConvertI64U => Instruction::F32ConvertI64U,
        wasmparser::Operator::F32DemoteF64 => Instruction::F32DemoteF64,
        wasmparser::Operator::F64ConvertI32S => Instruction::F64ConvertI32S,
        wasmparser::Operator::F64ConvertI32U => Instruction::F64ConvertI32U,
        wasmparser::Operator::F64ConvertI64S => Instruction::F64ConvertI64S,
        wasmparser::Operator::F64ConvertI64U => Instruction::F64ConvertI64U,
        wasmparser::Operator::F64PromoteF32 => Instruction::F64PromoteF32,
        wasmparser::Operator::I32ReinterpretF32 => Instruction::I32ReinterpretF32,
        wasmparser::Operator::I64ReinterpretF64 => Instruction::I64ReinterpretF64,
        wasmparser::Operator::F32ReinterpretI32 => Instruction::F32ReinterpretI32,
        wasmparser::Operator::F64ReinterpretI64 => Instruction::F64ReinterpretI64,
        wasmparser::Operator::I32Extend8S => Instruction::I32Extend8S,
        wasmparser::Operator::I32Extend16S => Instruction::I32Extend16S,
        wasmparser::Operator::I64Extend8S => Instruction::I64Extend8S,
        wasmparser::Operator::I64Extend16S => Instruction::I64Extend16S,
        wasmparser::Operator::I64Extend32S => Instruction::I64Extend32S,
        wasmparser::Operator::I32TruncSatF32S => Instruction::I32TruncSatF32S,
        wasmparser::Operator::I32TruncSatF32U => Instruction::I32TruncSatF32U,
        wasmparser::Operator::I32TruncSatF64S => Instruction::I32TruncSatF64S,
        wasmparser::Operator::I32TruncSatF64U => Instruction::I32TruncSatF64U,
        wasmparser::Operator::I64TruncSatF32S => Instruction::I64TruncSatF32S,
        wasmparser::Operator::I64TruncSatF32U => Instruction::I64TruncSatF32U,
        wasmparser::Operator::I64TruncSatF64S => Instruction::I64TruncSatF64S,
        wasmparser::Operator::I64TruncSatF64U => Instruction::I64TruncSatF64U,
        wasmparser::Operator::MemoryInit { .. } => todo!("memoryinit"),
        wasmparser::Operator::DataDrop { .. } => todo!("datadrop"),
        wasmparser::Operator::MemoryCopy { dst_mem, src_mem } => Instruction::MemoryCopy {
            src_mem: *src_mem,
            dst_mem: *dst_mem,
        },
        wasmparser::Operator::MemoryFill { mem } => Instruction::MemoryFill(*mem),
        wasmparser::Operator::TableInit { .. } => todo!("tableinit"),
        wasmparser::Operator::ElemDrop { .. } => todo!("elemdrop"),
        wasmparser::Operator::TableCopy { .. } => todo!("tablecopy"),
        wasmparser::Operator::TableFill { .. } => todo!("tablefill"),
        wasmparser::Operator::TableGet { .. } => todo!("tableget"),
        wasmparser::Operator::TableSet { .. } => todo!("tableset"),
        wasmparser::Operator::TableGrow { .. } => todo!("tablegrow"),
        wasmparser::Operator::TableSize { .. } => todo!("tablesize"),
        wasmparser::Operator::MemoryAtomicNotify { memarg } => {
            Instruction::MemoryAtomicNotify(memarg_map(memarg))
        }
        wasmparser::Operator::MemoryAtomicWait32 { memarg } => {
            Instruction::MemoryAtomicWait32(memarg_map(memarg))
        }
        wasmparser::Operator::MemoryAtomicWait64 { memarg } => {
            Instruction::MemoryAtomicWait64(memarg_map(memarg))
        }
        wasmparser::Operator::AtomicFence => Instruction::AtomicFence,
        wasmparser::Operator::I32AtomicLoad { memarg } => {
            Instruction::I32AtomicLoad(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicLoad { memarg } => {
            Instruction::I64AtomicLoad(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicLoad8U { memarg } => {
            Instruction::I32AtomicLoad8U(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicLoad16U { memarg } => {
            Instruction::I32AtomicLoad16U(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicLoad8U { memarg } => {
            Instruction::I64AtomicLoad8U(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicLoad16U { memarg } => {
            Instruction::I64AtomicLoad16U(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicLoad32U { memarg } => {
            Instruction::I64AtomicLoad32U(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicStore { memarg } => {
            Instruction::I32AtomicStore(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicStore { memarg } => {
            Instruction::I64AtomicStore(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicStore8 { memarg } => {
            Instruction::I32AtomicStore8(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicStore16 { memarg } => {
            Instruction::I32AtomicStore16(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicStore8 { memarg } => {
            Instruction::I64AtomicStore8(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicStore16 { memarg } => {
            Instruction::I64AtomicStore16(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicStore32 { memarg } => {
            Instruction::I64AtomicStore32(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmwAdd { memarg } => {
            Instruction::I32AtomicRmwAdd(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmwAdd { memarg } => {
            Instruction::I64AtomicRmwAdd(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmw8AddU { memarg } => {
            Instruction::I32AtomicRmw8AddU(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmw16AddU { memarg } => {
            Instruction::I32AtomicRmw16AddU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw8AddU { memarg } => {
            Instruction::I64AtomicRmw8AddU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw16AddU { memarg } => {
            Instruction::I64AtomicRmw16AddU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw32AddU { memarg } => {
            Instruction::I64AtomicRmw32AddU(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmwSub { memarg } => {
            Instruction::I32AtomicRmwSub(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmwSub { memarg } => {
            Instruction::I64AtomicRmwSub(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmw8SubU { memarg } => {
            Instruction::I32AtomicRmw8SubU(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmw16SubU { memarg } => {
            Instruction::I32AtomicRmw16SubU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw8SubU { memarg } => {
            Instruction::I64AtomicRmw8SubU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw16SubU { memarg } => {
            Instruction::I64AtomicRmw16SubU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw32SubU { memarg } => {
            Instruction::I64AtomicRmw32SubU(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmwAnd { memarg } => {
            Instruction::I32AtomicRmwAnd(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmwAnd { memarg } => {
            Instruction::I64AtomicRmwAnd(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmw8AndU { memarg } => {
            Instruction::I32AtomicRmw8AndU(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmw16AndU { memarg } => {
            Instruction::I32AtomicRmw16AndU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw8AndU { memarg } => {
            Instruction::I64AtomicRmw8AndU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw16AndU { memarg } => {
            Instruction::I64AtomicRmw16AndU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw32AndU { memarg } => {
            Instruction::I64AtomicRmw32AndU(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmwOr { memarg } => {
            Instruction::I32AtomicRmwOr(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmwOr { memarg } => {
            Instruction::I64AtomicRmwOr(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmw8OrU { memarg } => {
            Instruction::I32AtomicRmw8OrU(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmw16OrU { memarg } => {
            Instruction::I32AtomicRmw16OrU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw8OrU { memarg } => {
            Instruction::I64AtomicRmw8OrU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw16OrU { memarg } => {
            Instruction::I64AtomicRmw16OrU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw32OrU { memarg } => {
            Instruction::I64AtomicRmw32OrU(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmwXor { memarg } => {
            Instruction::I32AtomicRmwXor(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmwXor { memarg } => {
            Instruction::I64AtomicRmwXor(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmw8XorU { memarg } => {
            Instruction::I32AtomicRmw8XorU(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmw16XorU { memarg } => {
            Instruction::I32AtomicRmw16XorU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw8XorU { memarg } => {
            Instruction::I64AtomicRmw8XorU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw16XorU { memarg } => {
            Instruction::I64AtomicRmw16XorU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw32XorU { memarg } => {
            Instruction::I64AtomicRmw32XorU(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmwXchg { memarg } => {
            Instruction::I32AtomicRmwXchg(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmwXchg { memarg } => {
            Instruction::I64AtomicRmwXchg(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmw8XchgU { memarg } => {
            Instruction::I32AtomicRmw8XchgU(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmw16XchgU { memarg } => {
            Instruction::I32AtomicRmw16XchgU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw8XchgU { memarg } => {
            Instruction::I64AtomicRmw8XchgU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw16XchgU { memarg } => {
            Instruction::I64AtomicRmw16XchgU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw32XchgU { memarg } => {
            Instruction::I64AtomicRmw32XchgU(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmwCmpxchg { memarg } => {
            Instruction::I32AtomicRmwCmpxchg(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmwCmpxchg { memarg } => {
            Instruction::I64AtomicRmwCmpxchg(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmw8CmpxchgU { memarg } => {
            Instruction::I32AtomicRmw8CmpxchgU(memarg_map(memarg))
        }
        wasmparser::Operator::I32AtomicRmw16CmpxchgU { memarg } => {
            Instruction::I32AtomicRmw16CmpxchgU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw8CmpxchgU { memarg } => {
            Instruction::I64AtomicRmw8CmpxchgU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw16CmpxchgU { memarg } => {
            Instruction::I64AtomicRmw16CmpxchgU(memarg_map(memarg))
        }
        wasmparser::Operator::I64AtomicRmw32CmpxchgU { memarg } => {
            Instruction::I64AtomicRmw32CmpxchgU(memarg_map(memarg))
        }
        wasmparser::Operator::V128Load { memarg } => Instruction::V128Load(memarg_map(memarg)),
        wasmparser::Operator::V128Load8x8S { memarg } => {
            Instruction::V128Load8x8S(memarg_map(memarg))
        }
        wasmparser::Operator::V128Load8x8U { memarg } => {
            Instruction::V128Load8x8U(memarg_map(memarg))
        }
        wasmparser::Operator::V128Load16x4S { memarg } => {
            Instruction::V128Load16x4S(memarg_map(memarg))
        }
        wasmparser::Operator::V128Load16x4U { memarg } => {
            Instruction::V128Load16x4U(memarg_map(memarg))
        }
        wasmparser::Operator::V128Load32x2S { memarg } => {
            Instruction::V128Load32x2S(memarg_map(memarg))
        }
        wasmparser::Operator::V128Load32x2U { memarg } => {
            Instruction::V128Load32x2U(memarg_map(memarg))
        }
        wasmparser::Operator::V128Load8Splat { memarg } => {
            Instruction::V128Load8Splat(memarg_map(memarg))
        }
        wasmparser::Operator::V128Load16Splat { memarg } => {
            Instruction::V128Load16Splat(memarg_map(memarg))
        }
        wasmparser::Operator::V128Load32Splat { memarg } => {
            Instruction::V128Load32Splat(memarg_map(memarg))
        }
        wasmparser::Operator::V128Load64Splat { memarg } => {
            Instruction::V128Load64Splat(memarg_map(memarg))
        }
        wasmparser::Operator::V128Load32Zero { memarg } => {
            Instruction::V128Load32Zero(memarg_map(memarg))
        }
        wasmparser::Operator::V128Load64Zero { memarg } => {
            Instruction::V128Load64Zero(memarg_map(memarg))
        }
        wasmparser::Operator::V128Store { memarg } => Instruction::V128Store(memarg_map(memarg)),
        wasmparser::Operator::V128Load8Lane { .. } => todo!("lanes"),
        wasmparser::Operator::V128Load16Lane { .. } => todo!("lanes"),
        wasmparser::Operator::V128Load32Lane { .. } => todo!("lanes"),
        wasmparser::Operator::V128Load64Lane { .. } => todo!("lanes"),
        wasmparser::Operator::V128Store8Lane { .. } => todo!("lanes"),
        wasmparser::Operator::V128Store16Lane { .. } => todo!("lanes"),
        wasmparser::Operator::V128Store32Lane { .. } => todo!("lanes"),
        wasmparser::Operator::V128Store64Lane { .. } => todo!("lanes"),
        wasmparser::Operator::V128Const { .. } => todo!("lanes"),
        wasmparser::Operator::I8x16Shuffle { .. } => todo!("lanes"),
        wasmparser::Operator::I8x16ExtractLaneS { .. } => todo!("lanes"),
        wasmparser::Operator::I8x16ExtractLaneU { .. } => todo!("lanes"),
        wasmparser::Operator::I8x16ReplaceLane { .. } => todo!("lanes"),
        wasmparser::Operator::I16x8ExtractLaneS { .. } => todo!("lanes"),
        wasmparser::Operator::I16x8ExtractLaneU { .. } => todo!("lanes"),
        wasmparser::Operator::I16x8ReplaceLane { .. } => todo!("lanes"),
        wasmparser::Operator::I32x4ExtractLane { .. } => todo!("lanes"),
        wasmparser::Operator::I32x4ReplaceLane { .. } => todo!("lanes"),
        wasmparser::Operator::I64x2ExtractLane { .. } => todo!("lanes"),
        wasmparser::Operator::I64x2ReplaceLane { .. } => todo!("lanes"),
        wasmparser::Operator::F32x4ExtractLane { .. } => todo!("lanes"),
        wasmparser::Operator::F32x4ReplaceLane { .. } => todo!("lanes"),
        wasmparser::Operator::F64x2ExtractLane { .. } => todo!("lanes"),
        wasmparser::Operator::F64x2ReplaceLane { .. } => todo!("lanes"),
        wasmparser::Operator::I8x16Swizzle => Instruction::I8x16Swizzle,
        wasmparser::Operator::I8x16Splat => Instruction::I8x16Splat,
        wasmparser::Operator::I16x8Splat => Instruction::I16x8Splat,
        wasmparser::Operator::I32x4Splat => Instruction::I32x4Splat,
        wasmparser::Operator::I64x2Splat => Instruction::I64x2Splat,
        wasmparser::Operator::F32x4Splat => Instruction::F32x4Splat,
        wasmparser::Operator::F64x2Splat => Instruction::F64x2Splat,
        wasmparser::Operator::I8x16Eq => Instruction::I8x16Eq,
        wasmparser::Operator::I8x16Ne => Instruction::I8x16Ne,
        wasmparser::Operator::I8x16LtS => Instruction::I8x16LtS,
        wasmparser::Operator::I8x16LtU => Instruction::I8x16LtU,
        wasmparser::Operator::I8x16GtS => Instruction::I8x16GtS,
        wasmparser::Operator::I8x16GtU => Instruction::I8x16GtU,
        wasmparser::Operator::I8x16LeS => Instruction::I8x16LeS,
        wasmparser::Operator::I8x16LeU => Instruction::I8x16LeU,
        wasmparser::Operator::I8x16GeS => Instruction::I8x16GeS,
        wasmparser::Operator::I8x16GeU => Instruction::I8x16GeU,
        wasmparser::Operator::I16x8Eq => Instruction::I16x8Eq,
        wasmparser::Operator::I16x8Ne => Instruction::I16x8Ne,
        wasmparser::Operator::I16x8LtS => Instruction::I16x8LtS,
        wasmparser::Operator::I16x8LtU => Instruction::I16x8LtU,
        wasmparser::Operator::I16x8GtS => Instruction::I16x8GtS,
        wasmparser::Operator::I16x8GtU => Instruction::I16x8GtU,
        wasmparser::Operator::I16x8LeS => Instruction::I16x8LeS,
        wasmparser::Operator::I16x8LeU => Instruction::I16x8LeU,
        wasmparser::Operator::I16x8GeS => Instruction::I16x8GeS,
        wasmparser::Operator::I16x8GeU => Instruction::I16x8GeU,
        wasmparser::Operator::I32x4Eq => Instruction::I32x4Eq,
        wasmparser::Operator::I32x4Ne => Instruction::I32x4Ne,
        wasmparser::Operator::I32x4LtS => Instruction::I32x4LtS,
        wasmparser::Operator::I32x4LtU => Instruction::I32x4LtU,
        wasmparser::Operator::I32x4GtS => Instruction::I32x4GtS,
        wasmparser::Operator::I32x4GtU => Instruction::I32x4GtU,
        wasmparser::Operator::I32x4LeS => Instruction::I32x4LeS,
        wasmparser::Operator::I32x4LeU => Instruction::I32x4LeU,
        wasmparser::Operator::I32x4GeS => Instruction::I32x4GeS,
        wasmparser::Operator::I32x4GeU => Instruction::I32x4GeU,
        wasmparser::Operator::I64x2Eq => Instruction::I64x2Eq,
        wasmparser::Operator::I64x2Ne => Instruction::I64x2Ne,
        wasmparser::Operator::I64x2LtS => Instruction::I64x2LtS,
        wasmparser::Operator::I64x2GtS => Instruction::I64x2GtS,
        wasmparser::Operator::I64x2LeS => Instruction::I64x2LeS,
        wasmparser::Operator::I64x2GeS => Instruction::I64x2GeS,
        wasmparser::Operator::F32x4Eq => Instruction::F32x4Eq,
        wasmparser::Operator::F32x4Ne => Instruction::F32x4Ne,
        wasmparser::Operator::F32x4Lt => Instruction::F32x4Lt,
        wasmparser::Operator::F32x4Gt => Instruction::F32x4Gt,
        wasmparser::Operator::F32x4Le => Instruction::F32x4Le,
        wasmparser::Operator::F32x4Ge => Instruction::F32x4Ge,
        wasmparser::Operator::F64x2Eq => Instruction::F64x2Eq,
        wasmparser::Operator::F64x2Ne => Instruction::F64x2Ne,
        wasmparser::Operator::F64x2Lt => Instruction::F64x2Lt,
        wasmparser::Operator::F64x2Gt => Instruction::F64x2Gt,
        wasmparser::Operator::F64x2Le => Instruction::F64x2Le,
        wasmparser::Operator::F64x2Ge => Instruction::F64x2Ge,
        wasmparser::Operator::V128Not => Instruction::V128Not,
        wasmparser::Operator::V128And => Instruction::V128And,
        wasmparser::Operator::V128AndNot => Instruction::V128AndNot,
        wasmparser::Operator::V128Or => Instruction::V128Or,
        wasmparser::Operator::V128Xor => Instruction::V128Xor,
        wasmparser::Operator::V128Bitselect => Instruction::V128Bitselect,
        wasmparser::Operator::V128AnyTrue => Instruction::V128AnyTrue,
        wasmparser::Operator::I8x16Abs => Instruction::I8x16Abs,
        wasmparser::Operator::I8x16Neg => Instruction::I8x16Neg,
        wasmparser::Operator::I8x16Popcnt => Instruction::I8x16Popcnt,
        wasmparser::Operator::I8x16AllTrue => Instruction::I8x16AllTrue,
        wasmparser::Operator::I8x16Bitmask => Instruction::I8x16Bitmask,
        wasmparser::Operator::I8x16NarrowI16x8S => Instruction::I8x16NarrowI16x8S,
        wasmparser::Operator::I8x16NarrowI16x8U => Instruction::I8x16NarrowI16x8U,
        wasmparser::Operator::I8x16Shl => Instruction::I8x16Shl,
        wasmparser::Operator::I8x16ShrS => Instruction::I8x16ShrS,
        wasmparser::Operator::I8x16ShrU => Instruction::I8x16ShrU,
        wasmparser::Operator::I8x16Add => Instruction::I8x16Add,
        wasmparser::Operator::I8x16AddSatS => Instruction::I8x16AddSatS,
        wasmparser::Operator::I8x16AddSatU => Instruction::I8x16AddSatU,
        wasmparser::Operator::I8x16Sub => Instruction::I8x16Sub,
        wasmparser::Operator::I8x16SubSatS => Instruction::I8x16SubSatS,
        wasmparser::Operator::I8x16SubSatU => Instruction::I8x16SubSatU,
        wasmparser::Operator::I8x16MinS => Instruction::I8x16MinS,
        wasmparser::Operator::I8x16MinU => Instruction::I8x16MinU,
        wasmparser::Operator::I8x16MaxS => Instruction::I8x16MaxS,
        wasmparser::Operator::I8x16MaxU => Instruction::I8x16MaxU,
        wasmparser::Operator::I8x16AvgrU => Instruction::I8x16AvgrU,
        wasmparser::Operator::I16x8ExtAddPairwiseI8x16S => Instruction::I16x8ExtAddPairwiseI8x16S,
        wasmparser::Operator::I16x8ExtAddPairwiseI8x16U => Instruction::I16x8ExtAddPairwiseI8x16U,
        wasmparser::Operator::I16x8Abs => Instruction::I16x8Abs,
        wasmparser::Operator::I16x8Neg => Instruction::I16x8Neg,
        wasmparser::Operator::I16x8Q15MulrSatS => Instruction::I16x8Q15MulrSatS,
        wasmparser::Operator::I16x8AllTrue => Instruction::I16x8AllTrue,
        wasmparser::Operator::I16x8Bitmask => Instruction::I16x8Bitmask,
        wasmparser::Operator::I16x8NarrowI32x4S => Instruction::I16x8NarrowI32x4S,
        wasmparser::Operator::I16x8NarrowI32x4U => Instruction::I16x8NarrowI32x4U,
        wasmparser::Operator::I16x8ExtendLowI8x16S => Instruction::I16x8ExtendLowI8x16S,
        wasmparser::Operator::I16x8ExtendHighI8x16S => Instruction::I16x8ExtendHighI8x16S,
        wasmparser::Operator::I16x8ExtendLowI8x16U => Instruction::I16x8ExtendLowI8x16U,
        wasmparser::Operator::I16x8ExtendHighI8x16U => Instruction::I16x8ExtendHighI8x16U,
        wasmparser::Operator::I16x8Shl => Instruction::I16x8Shl,
        wasmparser::Operator::I16x8ShrS => Instruction::I16x8ShrS,
        wasmparser::Operator::I16x8ShrU => Instruction::I16x8ShrU,
        wasmparser::Operator::I16x8Add => Instruction::I16x8Add,
        wasmparser::Operator::I16x8AddSatS => Instruction::I16x8AddSatS,
        wasmparser::Operator::I16x8AddSatU => Instruction::I16x8AddSatU,
        wasmparser::Operator::I16x8Sub => Instruction::I16x8Sub,
        wasmparser::Operator::I16x8SubSatS => Instruction::I16x8SubSatS,
        wasmparser::Operator::I16x8SubSatU => Instruction::I16x8SubSatU,
        wasmparser::Operator::I16x8Mul => Instruction::I16x8Mul,
        wasmparser::Operator::I16x8MinS => Instruction::I16x8MinS,
        wasmparser::Operator::I16x8MinU => Instruction::I16x8MinU,
        wasmparser::Operator::I16x8MaxS => Instruction::I16x8MaxS,
        wasmparser::Operator::I16x8MaxU => Instruction::I16x8MaxU,
        wasmparser::Operator::I16x8AvgrU => Instruction::I16x8AvgrU,
        wasmparser::Operator::I16x8ExtMulLowI8x16S => Instruction::I16x8ExtMulLowI8x16S,
        wasmparser::Operator::I16x8ExtMulHighI8x16S => Instruction::I16x8ExtMulHighI8x16S,
        wasmparser::Operator::I16x8ExtMulLowI8x16U => Instruction::I16x8ExtMulLowI8x16U,
        wasmparser::Operator::I16x8ExtMulHighI8x16U => Instruction::I16x8ExtMulHighI8x16U,
        wasmparser::Operator::I32x4ExtAddPairwiseI16x8S => Instruction::I32x4ExtAddPairwiseI16x8S,
        wasmparser::Operator::I32x4ExtAddPairwiseI16x8U => Instruction::I32x4ExtAddPairwiseI16x8U,
        wasmparser::Operator::I32x4Abs => Instruction::I32x4Abs,
        wasmparser::Operator::I32x4Neg => Instruction::I32x4Neg,
        wasmparser::Operator::I32x4AllTrue => Instruction::I32x4AllTrue,
        wasmparser::Operator::I32x4Bitmask => Instruction::I32x4Bitmask,
        wasmparser::Operator::I32x4ExtendLowI16x8S => Instruction::I32x4ExtendLowI16x8S,
        wasmparser::Operator::I32x4ExtendHighI16x8S => Instruction::I32x4ExtendHighI16x8S,
        wasmparser::Operator::I32x4ExtendLowI16x8U => Instruction::I32x4ExtendLowI16x8U,
        wasmparser::Operator::I32x4ExtendHighI16x8U => Instruction::I32x4ExtendHighI16x8U,
        wasmparser::Operator::I32x4Shl => Instruction::I32x4Shl,
        wasmparser::Operator::I32x4ShrS => Instruction::I32x4ShrS,
        wasmparser::Operator::I32x4ShrU => Instruction::I32x4ShrU,
        wasmparser::Operator::I32x4Add => Instruction::I32x4Add,
        wasmparser::Operator::I32x4Sub => Instruction::I32x4Sub,
        wasmparser::Operator::I32x4Mul => Instruction::I32x4Mul,
        wasmparser::Operator::I32x4MinS => Instruction::I32x4MinS,
        wasmparser::Operator::I32x4MinU => Instruction::I32x4MinU,
        wasmparser::Operator::I32x4MaxS => Instruction::I32x4MaxS,
        wasmparser::Operator::I32x4MaxU => Instruction::I32x4MaxU,
        wasmparser::Operator::I32x4DotI16x8S => Instruction::I32x4DotI16x8S,
        wasmparser::Operator::I32x4ExtMulLowI16x8S => Instruction::I32x4ExtMulLowI16x8S,
        wasmparser::Operator::I32x4ExtMulHighI16x8S => Instruction::I32x4ExtMulHighI16x8S,
        wasmparser::Operator::I32x4ExtMulLowI16x8U => Instruction::I32x4ExtMulLowI16x8U,
        wasmparser::Operator::I32x4ExtMulHighI16x8U => Instruction::I32x4ExtMulHighI16x8U,
        wasmparser::Operator::I64x2Abs => Instruction::I64x2Abs,
        wasmparser::Operator::I64x2Neg => Instruction::I64x2Neg,
        wasmparser::Operator::I64x2AllTrue => Instruction::I64x2AllTrue,
        wasmparser::Operator::I64x2Bitmask => Instruction::I64x2Bitmask,
        wasmparser::Operator::I64x2ExtendLowI32x4S => Instruction::I64x2ExtendLowI32x4S,
        wasmparser::Operator::I64x2ExtendHighI32x4S => Instruction::I64x2ExtendHighI32x4S,
        wasmparser::Operator::I64x2ExtendLowI32x4U => Instruction::I64x2ExtendLowI32x4U,
        wasmparser::Operator::I64x2ExtendHighI32x4U => Instruction::I64x2ExtendHighI32x4U,
        wasmparser::Operator::I64x2Shl => Instruction::I64x2Shl,
        wasmparser::Operator::I64x2ShrS => Instruction::I64x2ShrS,
        wasmparser::Operator::I64x2ShrU => Instruction::I64x2ShrU,
        wasmparser::Operator::I64x2Add => Instruction::I64x2Add,
        wasmparser::Operator::I64x2Sub => Instruction::I64x2Sub,
        wasmparser::Operator::I64x2Mul => Instruction::I64x2Mul,
        wasmparser::Operator::I64x2ExtMulLowI32x4S => Instruction::I64x2ExtMulLowI32x4S,
        wasmparser::Operator::I64x2ExtMulHighI32x4S => Instruction::I64x2ExtMulHighI32x4S,
        wasmparser::Operator::I64x2ExtMulLowI32x4U => Instruction::I64x2ExtMulLowI32x4U,
        wasmparser::Operator::I64x2ExtMulHighI32x4U => Instruction::I64x2ExtMulHighI32x4U,
        wasmparser::Operator::F32x4Ceil => Instruction::F32x4Ceil,
        wasmparser::Operator::F32x4Floor => Instruction::F32x4Floor,
        wasmparser::Operator::F32x4Trunc => Instruction::F32x4Trunc,
        wasmparser::Operator::F32x4Nearest => Instruction::F32x4Nearest,
        wasmparser::Operator::F32x4Abs => Instruction::F32x4Abs,
        wasmparser::Operator::F32x4Neg => Instruction::F32x4Neg,
        wasmparser::Operator::F32x4Sqrt => Instruction::F32x4Sqrt,
        wasmparser::Operator::F32x4Add => Instruction::F32x4Add,
        wasmparser::Operator::F32x4Sub => Instruction::F32x4Sub,
        wasmparser::Operator::F32x4Mul => Instruction::F32x4Mul,
        wasmparser::Operator::F32x4Div => Instruction::F32x4Div,
        wasmparser::Operator::F32x4Min => Instruction::F32x4Min,
        wasmparser::Operator::F32x4Max => Instruction::F32x4Max,
        wasmparser::Operator::F32x4PMin => Instruction::F32x4PMin,
        wasmparser::Operator::F32x4PMax => Instruction::F32x4PMax,
        wasmparser::Operator::F64x2Ceil => Instruction::F64x2Ceil,
        wasmparser::Operator::F64x2Floor => Instruction::F64x2Floor,
        wasmparser::Operator::F64x2Trunc => Instruction::F64x2Trunc,
        wasmparser::Operator::F64x2Nearest => Instruction::F64x2Nearest,
        wasmparser::Operator::F64x2Abs => Instruction::F64x2Abs,
        wasmparser::Operator::F64x2Neg => Instruction::F64x2Neg,
        wasmparser::Operator::F64x2Sqrt => Instruction::F64x2Sqrt,
        wasmparser::Operator::F64x2Add => Instruction::F64x2Add,
        wasmparser::Operator::F64x2Sub => Instruction::F64x2Sub,
        wasmparser::Operator::F64x2Mul => Instruction::F64x2Mul,
        wasmparser::Operator::F64x2Div => Instruction::F64x2Div,
        wasmparser::Operator::F64x2Min => Instruction::F64x2Min,
        wasmparser::Operator::F64x2Max => Instruction::F64x2Max,
        wasmparser::Operator::F64x2PMin => Instruction::F64x2PMin,
        wasmparser::Operator::F64x2PMax => Instruction::F64x2PMax,
        wasmparser::Operator::I32x4TruncSatF32x4S => Instruction::I32x4TruncSatF32x4S,
        wasmparser::Operator::I32x4TruncSatF32x4U => Instruction::I32x4TruncSatF32x4U,
        wasmparser::Operator::F32x4ConvertI32x4S => Instruction::F32x4ConvertI32x4S,
        wasmparser::Operator::F32x4ConvertI32x4U => Instruction::F32x4ConvertI32x4U,
        wasmparser::Operator::I32x4TruncSatF64x2SZero => Instruction::I32x4TruncSatF64x2SZero,
        wasmparser::Operator::I32x4TruncSatF64x2UZero => Instruction::I32x4TruncSatF64x2UZero,
        wasmparser::Operator::F64x2ConvertLowI32x4S => Instruction::F64x2ConvertLowI32x4S,
        wasmparser::Operator::F64x2ConvertLowI32x4U => Instruction::F64x2ConvertLowI32x4U,
        wasmparser::Operator::F32x4DemoteF64x2Zero => Instruction::F32x4DemoteF64x2Zero,
        wasmparser::Operator::F64x2PromoteLowF32x4 => Instruction::F64x2PromoteLowF32x4,
        wasmparser::Operator::I8x16RelaxedSwizzle => Instruction::I8x16RelaxedSwizzle,
        wasmparser::Operator::I8x16RelaxedLaneselect => Instruction::I8x16RelaxedLaneselect,
        wasmparser::Operator::I16x8RelaxedLaneselect => Instruction::I16x8RelaxedLaneselect,
        wasmparser::Operator::I32x4RelaxedLaneselect => Instruction::I32x4RelaxedLaneselect,
        wasmparser::Operator::I64x2RelaxedLaneselect => Instruction::I64x2RelaxedLaneselect,
        wasmparser::Operator::F32x4RelaxedMin => Instruction::F32x4RelaxedMin,
        wasmparser::Operator::F32x4RelaxedMax => Instruction::F32x4RelaxedMax,
        wasmparser::Operator::F64x2RelaxedMin => Instruction::F64x2RelaxedMin,
        wasmparser::Operator::F64x2RelaxedMax => Instruction::F64x2RelaxedMax,
        wasmparser::Operator::I16x8RelaxedQ15mulrS => Instruction::I16x8RelaxedQ15mulrS,
        wasmparser::Operator::MemoryDiscard { .. } => todo!("memory discard"),
        wasmparser::Operator::I32x4RelaxedTruncF32x4S => Instruction::I32x4RelaxedTruncF32x4S,
        wasmparser::Operator::I32x4RelaxedTruncF32x4U => Instruction::I32x4RelaxedTruncF32x4U,
        wasmparser::Operator::I32x4RelaxedTruncF64x2SZero => {
            Instruction::I32x4RelaxedTruncF64x2SZero
        }
        wasmparser::Operator::I32x4RelaxedTruncF64x2UZero => {
            Instruction::I32x4RelaxedTruncF64x2UZero
        }
        wasmparser::Operator::F32x4RelaxedMadd => Instruction::F32x4RelaxedMadd,
        wasmparser::Operator::F32x4RelaxedNmadd => Instruction::F32x4RelaxedNmadd,
        wasmparser::Operator::F64x2RelaxedMadd => Instruction::F64x2RelaxedMadd,
        wasmparser::Operator::F64x2RelaxedNmadd => Instruction::F64x2RelaxedNmadd,
        wasmparser::Operator::I16x8RelaxedDotI8x16I7x16S => Instruction::I16x8RelaxedDotI8x16I7x16S,
        wasmparser::Operator::I32x4RelaxedDotI8x16I7x16AddS => {
            Instruction::I32x4RelaxedDotI8x16I7x16AddS
        }
        wasmparser::Operator::I31New => todo!(),
        wasmparser::Operator::I31GetS => todo!(),
        wasmparser::Operator::I31GetU => todo!(),
    }
}
