use anyhow::{Context, Result};
use clap::Parser;
use std::{env, error::Error, fs, path::PathBuf, time::SystemTime};
use wasi_virt::{VirtExit, WasiVirt};
use wasm_compose::composer::ComponentComposer;

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
    /// As defined in [`VirtOpts`]
    #[arg(short, long, verbatim_doc_comment)]
    config: Option<String>,

    /// Allow the component to exit
    #[arg(long)]
    allow_exit: Option<bool>,

    // STDIO
    /// Enable all stdio
    #[arg(long)]
    allow_stdio: Option<bool>,
    /// Enable stdin
    #[arg(long)]
    allow_stdin: Option<bool>,
    /// Enable stdout
    #[arg(long)]
    allow_stdout: Option<bool>,
    /// Enable stderr
    #[arg(long)]
    allow_stderr: Option<bool>,

    // ENV
    /// Allow host access to all environment variables, or to a specific comma-separated list of variable names.
    #[arg(long, num_args(0..), use_value_delimiter(true), require_equals(true), value_name("ENV_VAR"))]
    allow_env: Option<Vec<String>>,

    #[arg(short, long, use_value_delimiter(true), value_name("ENV=VAR"), value_parser = parse_key_val::<String, String>)]
    env: Option<Vec<(String, String)>>,

    // FS
    #[arg(long, value_name("preopen=hostpreopen"), value_parser = parse_key_val::<String, String>)]
    preopen: Option<Vec<(String, String)>>,

    #[arg(long, value_name("preopen=virtualdir"), value_parser = parse_key_val::<String, String>)]
    mount: Option<Vec<(String, String)>>,

    // CLOCKS

    // SOCKETS

    //
    /// Wasm binary to compose the virtualization with
    /// If not provided, the virtualization component itself will only generated.
    #[arg(required(false))]
    compose: Option<String>,

    /// Output virtualization component Wasm file
    #[arg(short, long)]
    out: String,
}

// parser for KEY=VAR env vars
fn parse_key_val<T, U>(s: &str) -> Result<(T, U), Box<dyn Error + Send + Sync + 'static>>
where
    T: std::str::FromStr,
    T::Err: Error + Send + Sync + 'static,
    U: std::str::FromStr,
    U::Err: Error + Send + Sync + 'static,
{
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid KEY=value: no `=` found in `{s}`"))?;
    Ok((s[..pos].parse()?, s[pos + 1..].parse()?))
}

fn timestamp() -> u64 {
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(n) => n.as_secs(),
        Err(_) => panic!(),
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    let mut virt_opts = if let Some(config) = &args.config {
        toml::from_str(&fs::read_to_string(&config)?)?
    } else {
        WasiVirt::default()
    };

    // By default, we virtualize all subsystems
    // This ensures full encapsulation in the default (no argument) case

    // stdio
    virt_opts.stdio().stdin(
        args.allow_stdin
            .unwrap_or(args.allow_stdio.unwrap_or(false)),
    );
    virt_opts.stdio().stdout(
        args.allow_stdout
            .unwrap_or(args.allow_stdio.unwrap_or(false)),
    );
    virt_opts.stdio().stderr(
        args.allow_stderr
            .unwrap_or(args.allow_stdio.unwrap_or(false)),
    );

    // exit
    virt_opts.exit(if args.allow_exit.unwrap_or_default() {
        VirtExit::Passthrough
    } else {
        Default::default()
    });

    // env options
    let env = virt_opts.env();
    match args.allow_env {
        Some(allow_env) if allow_env.len() == 0 => {
            env.allow_all();
        }
        Some(allow_env) => {
            env.allow(&allow_env);
        }
        None => {}
    };
    if let Some(env_overrides) = args.env {
        env.overrides = env_overrides;
    }

    // fs options
    let fs = virt_opts.fs();
    if let Some(preopens) = args.preopen {
        for (preopen, hostpreopen) in preopens {
            fs.host_preopen(preopen, hostpreopen);
        }
    }
    if let Some(mounts) = args.mount {
        for (preopen, mountdir) in mounts {
            fs.virtual_preopen(preopen, mountdir);
        }
    }

    let virt_component = virt_opts.finish()?;

    let out_path = PathBuf::from(args.out);

    let out_bytes = if let Some(compose_path) = args.compose {
        let compose_path = PathBuf::from(compose_path);
        let dir = env::temp_dir();
        let tmp_virt = dir.join(format!("virt.{}.wasm", timestamp()));
        fs::write(&tmp_virt, virt_component.adapter)?;

        let composed_bytes = ComponentComposer::new(
            &compose_path,
            &wasm_compose::config::Config {
                definitions: vec![tmp_virt.clone()],
                ..Default::default()
            },
        )
        .compose()
        .with_context(|| "Unable to compose virtualized adapter into component.\nMake sure virtualizations are enabled and being used.")
        .or_else(|e| {
            fs::remove_file(&tmp_virt)?;
            Err(e)
        })?;

        fs::remove_file(&tmp_virt)?;

        composed_bytes
    } else {
        virt_component.adapter
    };

    if virt_component.virtual_files.len() > 0 {
        println!("Virtualized files from local filesystem:\n");
        for (virtual_path, original_path) in virt_component.virtual_files {
            println!("  - {virtual_path} : {original_path}");
        }
    }

    fs::write(&out_path, out_bytes)?;

    Ok(())
}
