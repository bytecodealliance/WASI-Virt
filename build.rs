use anyhow::{anyhow, Result};
use std::{env, process::Command};

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

fn main() -> Result<()> {
    if env::var("BUILDING_VIRT").is_err() {
        env::set_var("BUILDING_VIRT", "1");
        cmd("cargo +nightly build -p virtual-adapter --target wasm32-wasi --release -Z build-std=std,panic_abort -Z build-std-features=panic_immediate_abort")?;
        cmd("cp target/wasm32-wasi/release/virtual_adapter.wasm lib/")?;
    }

    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=virtual-adapter/Cargo.toml");
    println!("cargo:rerun-if-changed=virtual-adapter/src/lib.rs");
    println!("cargo:rerun-if-changed=build.rs");
    Ok(())
}
