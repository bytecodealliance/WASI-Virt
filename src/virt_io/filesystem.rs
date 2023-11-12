use anyhow::{Context, Result};
use walrus::Module;

use super::StubRequirement;

/// Imports exposed by WASI for Filesystem functionality
///
/// Some are allowed to be missing, some are required depending on
/// whether the FS is used or not (`fs_used` in `stub_fs_virt`)
const WASI_FS_IMPORTS: &[(&str, &str, &StubRequirement)] = &[
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.access-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.advise",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.append-via-stream",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.change-directory-permissions-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.change-file-permissions-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.create-directory-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.get-flags",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.link-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.lock-exclusive",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.lock-shared",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.metadata-hash",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.metadata-hash-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.read-directory",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.readlink-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.remove-directory-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.rename-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.set-size",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.set-times",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.set-times-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.symlink-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.sync",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.sync-data",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.try-lock-exclusive",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.try-lock-shared",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.unlink-file-at",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.unlock",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.write",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.write-via-stream",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/preopens@0.2.0-rc-2023-10-18",
        "get-directories",
        &StubRequirement::DependsOnFsUsage,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[resource-drop]descriptor",
        &StubRequirement::DependsOnFsUsage,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[resource-drop]directory-entry-stream",
        &StubRequirement::DependsOnFsUsage,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.get-type",
        &StubRequirement::DependsOnFsUsage,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.is-same-object",
        &StubRequirement::DependsOnFsUsage,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.open-at",
        &StubRequirement::DependsOnFsUsage,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.read",
        &StubRequirement::Optional,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]directory-entry-stream.read-directory-entry",
        &StubRequirement::DependsOnFsUsage,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.read-via-stream",
        &StubRequirement::DependsOnFsUsage,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.stat",
        &StubRequirement::DependsOnFsUsage,
    ),
    (
        "wasi:filesystem/types@0.2.0-rc-2023-10-18",
        "[method]descriptor.stat-at",
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

const WASI_FILESYSTEM_EXPORTS: &[&str] = &[
    "wasi:filesystem/preopens@0.2.0-rc-2023-10-18#get-directories",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.read-via-stream",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.write-via-stream",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.append-via-stream",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.advise",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.sync-data",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.get-flags",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.get-type",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.set-size",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.set-times",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.read",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.write",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.read-directory",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.sync",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.create-directory-at",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.stat",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.stat-at",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.set-times-at",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.link-at",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.open-at",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.readlink-at",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.remove-directory-at",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.rename-at",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.symlink-at",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.access-at",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.unlink-file-at",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.change-file-permissions-at",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.change-directory-permissions-at",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.lock-shared",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.lock-exclusive",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.try-lock-shared",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.try-lock-exclusive",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.unlock",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.metadata-hash",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.metadata-hash-at",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]descriptor.is-same-object",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[dtor]descriptor",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[method]directory-entry-stream.read-directory-entry",
    "wasi:filesystem/types@0.2.0-rc-2023-10-18#[dtor]directory-entry-stream",
];

/// Strip exported WASI functions that implement filesystem access
pub(crate) fn strip_fs_virt(module: &mut Module) -> Result<()> {
    stub_fs_virt(module, false)?;

    for &export_name in WASI_FILESYSTEM_EXPORTS {
        module
            .exports
            .remove(export_name)
            .with_context(|| format!("failed to strip WASI FS function [{export_name}]"))?;
    }

    Ok(())
}
