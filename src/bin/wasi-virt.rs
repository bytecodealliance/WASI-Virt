use anyhow::Result;
use clap::Parser;
use std::fs;
use wasi_virt::{create_virt, VirtOpts};

#[derive(Parser, Debug)]
#[command(verbatim_doc_comment, author, version, about)]
/// WASI Virt CLI
///
/// Creates a virtualization component with the provided virtualization configuration.
///
/// This virtualization component can then be composed into a WASI component via:
///
///   wasm-tools compose component.wasm -d virt.wasm -o final.wasm
///
struct Args {
    /// Virtualization TOML configuration
    ///
    /// Example configuration:
    ///  
    ///   [env]
    ///   host = "All" # or "None"
    ///   overrides = [["CUSTOM", "VAL"]]
    ///
    /// Alternatively, allow or deny env keys for the host can be configured via:
    ///
    ///   [env.host]
    ///   Allow = ["ENV_KEY"] # Or Deny = ...
    ///
    #[arg(short, long, verbatim_doc_comment)]
    config: String,

    /// Output virtualization component Wasm file
    #[arg(short, long)]
    out: String,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let virt_cfg: VirtOpts = toml::from_str(&fs::read_to_string(&args.config)?)?;

    let virt_component = create_virt(&virt_cfg)?;

    fs::write(args.out, virt_component.adapter)?;

    Ok(())
}
