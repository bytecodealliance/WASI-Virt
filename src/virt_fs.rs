use std::collections::{BTreeMap, HashMap};

use anyhow::{anyhow, Result};
use serde::Deserialize;
use walrus::{
    ir::{Const, Instr, Value},
    ActiveData, ActiveDataLocation, DataKind, ExportItem, FunctionKind, Module,
};

use crate::{
    data::DataSection,
    walrus_ops::{
        bump_stack_global, get_active_data_segment, get_stack_global, stub_imported_func,
    },
    WasiVirt,
};

impl WasiVirt {}

#[derive(Deserialize, Debug, Default, Clone)]
pub struct VirtFs {
    /// Filesystem state to virtualize
    preopens: BTreeMap<String, FsEntry>,
    /// A cutoff size in bytes, above which
    /// files will be treated as passive segments.
    /// Per-file control may also be provided.
    passive_cutoff: Option<u32>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub enum FsEntry {
    /// symlink absolute or relative file path on the virtual filesystem
    Symlink(String),
    /// host path at virtualization time
    Host(String),
    /// host path st runtime
    Runtime(String),
    /// Virtual file
    File(VirtFile),
    /// Virtual directory
    Dir(VirtDir),
}

#[derive(Deserialize, Debug, Clone)]
pub struct VirtFile {
    perms: Option<u16>,
    bytes: Option<Vec<u8>>,
    source: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct VirtDir {
    perms: Option<u16>,
    /// Inline directory definition
    entries: BTreeMap<String, FsEntry>,
}

#[repr(C)]
struct StaticIndexEntry {
    name: *const u8,
    ty: StaticIndexType,
    perms: u16,
    data: StaticFileData,
}

#[repr(u16)]
enum StaticIndexType {
    ActiveFile,
    PassiveFile,
    Dir,
    RuntimeHostDir,
    RuntimeHostFile,
}

#[repr(C)]
union StaticFileData {
    /// Active memory data pointer for ActiveFile
    active: *const u8,
    /// Passive memory element index for PassiveFile
    passive: u32,
    /// Host path string for HostDir / HostFile
    path: *const u8,
    // Pointer and child entry count for Dir
    dir: (*const StaticIndexEntry, u32),
}

fn visit_pre_mut<'a>(entry: &'a mut FsEntry, visit: fn(name: &str, entry: &mut FsEntry) -> ()) {
    match entry {
        FsEntry::Symlink(_) => todo!(),
        FsEntry::Host(_) => todo!(),
        FsEntry::Runtime(_) => todo!(),
        FsEntry::File(_) => todo!(),
        FsEntry::Dir(dir) => {
            for (name, sub_entry) in dir.entries.iter_mut() {
                visit(name, sub_entry);
            }
            for sub_entry in dir.entries.values_mut() {
                visit_pre_mut(sub_entry, visit);
            }
        }
    }
}

fn visit_pre<'a, Visitor>(entry: &'a FsEntry, visit: &mut Visitor)
where
    Visitor: FnMut(&str, &FsEntry, &'a VirtDir) -> (),
{
    match entry {
        FsEntry::Symlink(_) => todo!(),
        FsEntry::Host(_) => todo!(),
        FsEntry::Runtime(_) => todo!(),
        FsEntry::File(_) => todo!(),
        FsEntry::Dir(dir) => {
            for (name, sub_entry) in &dir.entries {
                visit(name, sub_entry, dir);
            }
            for sub_entry in dir.entries.values() {
                visit_pre(sub_entry, visit);
            }
        }
    }
}

pub fn create_fs_virt<'a>(module: &'a mut Module, fs: &VirtFs) -> Result<()> {
    // First we iterate the options and fill in all HostDir and HostFile entries
    // With InlineDir and InlineFile entries
    let mut fs = fs.clone();
    for entry in fs.preopens.values_mut() {
        match entry {
            FsEntry::Symlink(_) => todo!(),
            FsEntry::Runtime(_) => todo!(),
            FsEntry::Host(_) => todo!(),
            FsEntry::File(_) => todo!(),
            FsEntry::Dir(_) => {
                visit_pre_mut(entry, |name, entry| match entry {
                    FsEntry::File(file) => {}
                    FsEntry::Dir(dir) => {}
                    FsEntry::Symlink(_) => todo!(),
                    FsEntry::Host(_) => todo!(),
                    FsEntry::Runtime(_) => todo!(),
                });
            }
        }
    }

    // Create the data section bytes
    let mut data_section = DataSection::new(get_stack_global(module)? as usize);

    // Next we linearize the pre-order directory graph as the static file data
    // Using a pre-order traversal
    // Each parent node is formed along with its child length and deep subgraph
    // length.
    let mut static_fs_data: Vec<StaticIndexEntry> = Vec::new();

    let mut len = 0;

    // let mut last_parent: &DirEntry;
    let mut cur_child_cnt = 0;
    for (name, entry) in &fs.preopens {
        visit_pre(entry, &mut |name, entry, parent| {
            // if std::ptr::eq(parent, last_parent) {
            //     cur_child_cnt += 1;

            // } else {
            //     last_parent = parent;
            // }
            let name_str_ptr = data_section.string(name);
            let perms = 0x0;
            let (ty, data) = match &entry {
                FsEntry::Symlink(_) => todo!(),
                FsEntry::Host(_) => todo!(),
                FsEntry::Runtime(_) => todo!(),
                FsEntry::Dir(_) => todo!(),
                FsEntry::File(VirtFile {
                    perms,
                    bytes,
                    source,
                }) => (
                    StaticIndexType::Dir,
                    StaticFileData {
                        dir: (std::ptr::null(), 0),
                    },
                ),
            };
            static_fs_data.push(StaticIndexEntry {
                name: name_str_ptr,
                ty,
                perms,
                data,
            });
        });
        // assert!(stack.len() == 0);
    }

    // We then write the strings section

    // Followed by the static file section

    // Followed by the active file section

    // prepare the field data list vector for writing
    // strings must be sorted as binary searches are used against this data

    // for (key, value) in &fs.overrides {
    //     field_data_vec.push(key.as_ref());
    //     field_data_vec.push(value.as_ref());
    // }
    // field_data_vec.sort();
    // match &fs.host {
    //     HostEnv::Allow(allow_list) => {
    //         let mut allow_list: Vec<&str> = allow_list.iter().map(|item| item.as_ref()).collect();
    //         allow_list.sort();
    //         for key in allow_list {
    //             field_data_vec.push(key);
    //         }
    //     }
    //     HostEnv::Deny(deny_list) => {
    //         let mut deny_list: Vec<&str> = deny_list.iter().map(|item| item.as_ref()).collect();
    //         deny_list.sort();
    //         for key in deny_list {
    //             field_data_vec.push(key);
    //         }
    //     }
    //     _ => {}
    // }

    let mut field_data_bytes: Vec<u8> = Vec::new();
    // for str in field_data_vec {
    //     assert!(field_data_bytes.len() % 4 == 0);
    //     // write the length at the aligned offset
    //     // field_data_bytes.extend_from_slice(&(str.len() as u32).to_le_bytes());
    //     // let str_bytes = str.as_bytes();
    //     // field_data_bytes.extend_from_slice(str_bytes);
    //     // let rem = str_bytes.len() % 4;
    //     // // add padding for alignment if necessary
    //     // if rem > 0 {
    //     //     field_data_bytes.extend((0..4 - rem).map(|_| 0));
    //     // }
    // }

    let memory = module.memories.iter().nth(0).unwrap().id();

    let field_data_addr = if field_data_bytes.len() > 0 {
        // Offset the stack global by the static field data length
        let field_data_addr = bump_stack_global(module, field_data_bytes.len() as i32)?;

        // Add a new data segment for this new range created at the top of the stack
        module.data.add(
            DataKind::Active(ActiveData {
                memory,
                location: ActiveDataLocation::Absolute(field_data_addr),
            }),
            field_data_bytes,
        );
        Some(field_data_addr)
    } else {
        None
    };

    let fs_ptr_addr = {
        let fs_ptr_export = module
            .exports
            .iter()
            .find(|expt| expt.name.as_str() == "get_fs_ptr")
            .unwrap();
        let ExportItem::Function(func) = fs_ptr_export.item else {
            panic!()
        };
        let FunctionKind::Local(local_func) = &module.funcs.get(func).kind else {
            panic!()
        };
        let func_body = local_func.block(local_func.entry_block());
        if func_body.instrs.len() != 1 {
            return Err(anyhow!(
                "Unexpected get_fs_ptr implementation. Should be a constant address return."
            ));
        }
        let Instr::Const(Const {
            value: Value::I32(fs_ptr_addr),
        }) = &func_body.instrs[0].0
        else {
            return Err(anyhow!(
                "Unexpected get_fs_ptr implementation. Should be a constant address return."
            ));
        };
        *fs_ptr_addr as u32
    };

    // // If host env is disabled, remove its import entirely
    // // replacing it with a stub panic
    // if matches!(fs.host, HostEnv::None) {
    //     stub_imported_func(module, "wasi:cli-base/environment", "get-environment");
    //     // we do arguments as well because virt assumes reactors for now...
    //     stub_imported_func(module, "wasi:cli-base/environment", "get-arguments");
    // }

    // In the existing static data segment, update the static data options.
    //
    // From virtual-adapter/src/lib.js:
    //
    // #[repr(C)]
    // pub struct Env {
    //     /// Whether to fallback to the host env
    //     /// [byte 0]
    //     host_fallback: bool,
    //     /// Whether we are providing an allow list or a deny list
    //     /// on the fallback lookups
    //     /// [byte 1]
    //     host_fallback_allow: bool,
    //     /// How many host fields are defined in the data pointer
    //     /// [byte 4]
    //     host_field_cnt: u32,
    //     /// Host many host fields are defined to be allow or deny
    //     /// (these are concatenated at the end of the data with empty values)
    //     /// [byte 8]
    //     host_allow_or_deny_cnt: u32,
    //     /// Byte data of u32 byte len followed by string bytes
    //     /// up to the lengths previously provided.
    //     /// [byte 12]
    //     host_field_data: *const u8,
    // }
    let (data, data_offset) = get_active_data_segment(module, memory, fs_ptr_addr)?;
    let bytes = data.value.as_mut_slice();

    // let host_field_cnt = fs.overrides.len() as u32;
    // bytes[data_offset + 4..data_offset + 8].copy_from_slice(&host_field_cnt.to_le_bytes());
    // match &fs.host {
    //     // All is already the default data
    //     HostEnv::All => {}
    //     HostEnv::None => {
    //         bytes[data_offset] = 0;
    //     }
    //     HostEnv::Allow(allow_list) => {
    //         bytes[data_offset + 1] = 1;
    //         bytes[data_offset + 8..data_offset + 12]
    //         .copy_from_slice(&(allow_list.len() as u32).to_le_bytes());
    //     }
    //     HostEnv::Deny(deny_list) => {
    //         bytes[data_offset + 1] = 0;
    //         bytes[data_offset + 8..data_offset + 12]
    //         .copy_from_slice(&(deny_list.len() as u32).to_le_bytes());
    //     }
    // };
    // if let Some(field_data_addr) = field_data_addr {
    //     bytes[data_offset + 12..data_offset + 16].copy_from_slice(&field_data_addr.to_le_bytes());
    // }

    Ok(())
}
