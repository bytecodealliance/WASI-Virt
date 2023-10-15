use anyhow::{Context, Result};
use walrus::Module;

use super::StubRequirement;

/// Imports exposed by WASI for Filesystem functionality
///
/// Some are allowed to be missing, some are required depending on
/// whether the FS is used or not (`fs_used` in `stub_fs_virt`)
const WASI_FS_IMPORTS: [(&str, &str, &StubRequirement); 39] = [
    (
        "wasi:filesystem/types",
        "access-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "advise",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "append-via-stream",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "change-directory-permissions-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "change-file-permissions-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "create-directory-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "get-flags",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "link-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "lock-exclusive",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "lock-shared",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "metadata-hash",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "metadata-hash-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "read-directory",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "readlink-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "remove-directory-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "rename-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "set-size",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "set-times",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "set-times-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "symlink-at",
        &StubRequirement::Optional,
    ),
    ("wasi:filesystem/types", "sync", &StubRequirement::Optional),
    (
        "wasi:filesystem/types",
        "sync-data",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "try-lock-exclusive",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "try-lock-shared",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "unlink-file-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types",
        "unlock",
        &StubRequirement::Optional,
    ),
    ("wasi:filesystem/types", "write", &StubRequirement::Optional),
    (
        "wasi:filesystem/types",
        "write-via-stream",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/preopens",
        "get-directories",
        &StubRequirement::DependsOnFsUsage,
    ),
    (
        "wasi:filesystem/types",
        "drop-descriptor",
        &StubRequirement::DependsOnFsUsage,
    ),
    (
        "wasi:filesystem/types",
        "drop-directory-entry-stream",
        &StubRequirement::DependsOnFsUsage,
    ),
    (
        "wasi:filesystem/types",
        "get-type",
        &StubRequirement::DependsOnFsUsage,
    ),
    (
        "wasi:filesystem/types",
        "is-same-object",
        &StubRequirement::DependsOnFsUsage,
    ),
    (
        "wasi:filesystem/types",
        "open-at",
        &StubRequirement::DependsOnFsUsage,
    ),
    (
        "wasi:filesystem/types",
        "read",
        &StubRequirement::DependsOnFsUsage,
    ),
    (
        "wasi:filesystem/types",
        "read-directory-entry",
        &StubRequirement::DependsOnFsUsage,
    ),
    (
        "wasi:filesystem/types",
        "read-via-stream",
        &StubRequirement::DependsOnFsUsage,
    ),
    (
        "wasi:filesystem/types",
        "stat",
        &StubRequirement::DependsOnFsUsage,
    ),
    (
        "wasi:filesystem/types",
        "stat-at",
        &StubRequirement::DependsOnFsUsage,
    ),
];

/// Replace imported WASI functions that implement filesystem access with no-ops
// Stubs must be _comprehensive_ in order to act as full deny over entire subsystem
// when stubbing functions that are not part of the virtual adapter exports, we therefore
// have to create this functions fresh.
// Ideally, we should generate these stubs automatically from WASI definitions.
pub(crate) fn stub_fs_virt(module: &mut Module, uses_fs: bool) -> Result<()> {
    // Replace the filesystem functions that are allowed to be missing
    for (module_name, func_name, stub_requirement) in WASI_FS_IMPORTS {
        match (stub_requirement, uses_fs) {
            // If the stub is always required, or depends on FS usage and uses_fs is set
            // then we *must* find the function and replace it
            (StubRequirement::Required, _) | (StubRequirement::DependsOnFsUsage, true) => {
                let fid = module.imports.get_func(module_name, func_name)
                    .with_context(|| format!("failed to find required filesystem import [{func_name}] in module [{module_name}]"))?;

                module
                    .replace_imported_func(fid, |(body, _)| {
                        body.unreachable();
                    })
                    .with_context(|| {
                        "failed to stub filesystem functionality [{}] in module [{export_name}]"
                    })?;
            }
            // If the stub is optional, or required w/ FS usage and fs is not used, we can replace
            // the functions optimistically, and not fail if they are missing
            (StubRequirement::Optional, _) | (StubRequirement::DependsOnFsUsage, false) => {
                if let Ok(fid) = module.imports.get_func(module_name, func_name) {
                    module
                        .replace_imported_func(fid, |(body, _)| {
                            body.unreachable();
                        })
                        .with_context(|| {
                            "failed to stub filesystem functionality [{}] in module [{export_name}]"
                        })?;
                }
            }
        };
    }
    Ok(())
}

const WASI_FILESYSTEM_EXPORTS: [&str; 36] = [
    "wasi:filesystem/preopens#get-directories",
    "wasi:filesystem/types#read-via-stream",
    "wasi:filesystem/types#write-via-stream",
    "wasi:filesystem/types#append-via-stream",
    "wasi:filesystem/types#advise",
    "wasi:filesystem/types#sync-data",
    "wasi:filesystem/types#get-flags",
    "wasi:filesystem/types#get-type",
    "wasi:filesystem/types#set-size",
    "wasi:filesystem/types#set-times",
    "wasi:filesystem/types#read",
    "wasi:filesystem/types#write",
    "wasi:filesystem/types#read-directory",
    "wasi:filesystem/types#sync",
    "wasi:filesystem/types#create-directory-at",
    "wasi:filesystem/types#stat",
    "wasi:filesystem/types#stat-at",
    "wasi:filesystem/types#set-times-at",
    "wasi:filesystem/types#link-at",
    "wasi:filesystem/types#open-at",
    "wasi:filesystem/types#readlink-at",
    "wasi:filesystem/types#remove-directory-at",
    "wasi:filesystem/types#rename-at",
    "wasi:filesystem/types#symlink-at",
    "wasi:filesystem/types#access-at",
    "wasi:filesystem/types#unlink-file-at",
    "wasi:filesystem/types#change-file-permissions-at",
    "wasi:filesystem/types#change-directory-permissions-at",
    "wasi:filesystem/types#lock-shared",
    "wasi:filesystem/types#lock-exclusive",
    "wasi:filesystem/types#try-lock-shared",
    "wasi:filesystem/types#try-lock-exclusive",
    "wasi:filesystem/types#unlock",
    "wasi:filesystem/types#drop-descriptor",
    "wasi:filesystem/types#read-directory-entry",
    "wasi:filesystem/types#drop-directory-entry-stream",
];

/// Strip exported WASI functions that implement filesystem access
pub(crate) fn strip_fs_virt(module: &mut Module) -> Result<()> {
    stub_fs_virt(module, false)?;

    for export_name in WASI_FILESYSTEM_EXPORTS {
        module
            .exports
            .remove(export_name)
            .with_context(|| format!("failed to strip WASI FS function [{export_name}]"))?;
    }

    Ok(())
}
