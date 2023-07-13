use anyhow::{anyhow, Result};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use wit_component::{self, StringEncoding};
use wit_parser::Resolve;

fn cmd(arg: &str) -> Result<()> {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C");
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.arg("-c");
        cmd
    };
    let output = cmd.arg(arg).output()?;
    if !output.status.success() {
        return Err(anyhow!(
            "failed running command: {}\n{}",
            arg,
            &String::from_utf8(output.stderr)?
        ));
    }
    Ok(())
}

fn create_world_encoding(wit_path: &str, world_name: &str, out_file: &str) -> Result<()> {
    let mut resolve = Resolve::default();

    let (id, _) = resolve.push_dir(&PathBuf::from(wit_path))?;

    let world = resolve.select_world(id, Some(world_name))?;

    let encoded = wit_component::metadata::encode(&resolve, world, StringEncoding::UTF8, None)?;
    fs::write(out_file, encoded)?;
    Ok(())
}

fn main() -> Result<()> {
    if env::var("BUILDING_VIRT").is_ok() {
        return Ok(());
    }
    env::set_var("BUILDING_VIRT", "1");

    // build the main virtual adapter
    cmd("cargo +nightly build -p virtual-adapter --target wasm32-wasi --release -Z build-std=std,panic_abort -Z build-std-features=panic_immediate_abort")?;
    cmd("cp target/wasm32-wasi/release/virtual_adapter.wasm lib/")?;

    // we also build dummy components for each of the virt subsystems
    // this way we can merge only the used worlds
    create_world_encoding("wit", "virtual-env", "lib/virtual_env.obj")?;
    create_world_encoding("wit", "virtual-fs", "lib/virtual_fs.obj")?;

    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=virtual-adapter/Cargo.toml");
    println!("cargo:rerun-if-changed=virtual-adapter/src/lib.rs");
    println!("cargo:rerun-if-changed=virtual-adapter/src/fs.rs");
    println!("cargo:rerun-if-changed=virtual-adapter/src/env.rs");
    println!("cargo:rerun-if-changed=wit/virt.wit");
    println!("cargo:rerun-if-changed=build.rs");
    Ok(())
}
