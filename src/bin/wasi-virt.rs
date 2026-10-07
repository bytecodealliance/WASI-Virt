use std::error::Error;
use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{ArgAction, Parser};
use semver::Version;

use wasi_virt::{StdioCfg, WasiVirt, DEFAULT_INSERT_WASI_VERSION};

#[derive(Parser, Debug)]
#[command(verbatim_doc_comment, author, version, about, long_about = None)]
/// WASI Virt
struct Args {
    /// Optional Wasm binary to compose the virtualization into.
    /// If not provided, only the virtualization component itself will be generated,
    /// which can then be composed via `wasm-tools compose -d virt.wasm component.wasm`
    #[arg(required(false), value_name("component.wasm"), verbatim_doc_comment)]
    compose: Option<PathBuf>,

    /// Enable debug tracing of all virtualized calls
    #[arg(long, action = ArgAction::SetTrue)]
    debug: Option<bool>,

    /// Output virtualization component Wasm file
    #[arg(short, long, value_name("virt.wasm"))]
    out: String,

    /// Enable all subsystem passthrough (encapsulation is the default)
    #[arg(long)]
    allow_all: bool,

    // CLOCKS
    /// Enable clocks
    #[arg(long, default_missing_value="true", num_args=0..=1)]
    allow_clocks: Option<bool>,

    /// Allow the component to exit
    #[arg(long, default_missing_value="true", num_args=0..=1)]
    allow_exit: Option<bool>,

    // HTTP
    /// Enable HTTP
    #[arg(long, default_missing_value="true", num_args=0..=1)]
    allow_http: Option<bool>,

    // RANDOM
    /// Enable Random
    #[arg(long, default_missing_value="true", num_args=0..=1)]
    allow_random: Option<bool>,

    // SOCKETS
    /// Enable Sockets
    #[arg(long, default_missing_value="true", num_args=0..=1)]
    allow_sockets: Option<bool>,

    // ENV
    /// Allow unrestricted access to host environment variables, or to a comma-separated list of variable names.
    #[arg(long, num_args(0..), use_value_delimiter(true), require_equals(true), value_name("ENV_VAR"), help_heading = "Env")]
    allow_env: Option<Vec<String>>,

    /// Set an environment variable override; repeat it for each variable
    #[arg(short, long, value_name("ENV=VAR"), value_parser = parse_key_val::<String, String>, help_heading = "Env")]
    env: Option<Vec<(String, String)>>,

    // CONFIG
    /// Allow unrestricted access to host  configuration properties, or to a comma-separated list of property names.
    #[arg(long, num_args(0..), use_value_delimiter(true), require_equals(true), value_name("PROPERTY_NAME"), help_heading = "Config (experimental)")]
    allow_config: Option<Vec<String>>,

    /// Set a config property override; repeat it for each property
    #[arg(short, long, value_name("NAME=VALUE"), value_parser = parse_key_val::<String, String>, help_heading = "Config (experimental)")]
    config: Option<Vec<(String, String)>>,

    // FS
    /// Allow unrestricted access to host preopens
    #[arg(long, default_missing_value="true", num_args=0..=1, help_heading = "Fs")]
    allow_fs: Option<bool>,

    /// Mount a virtual directory globbed from the local filesystem
    #[arg(long, value_name("preopen=virtualdir"), value_parser = parse_key_val::<String, String>, help_heading = "Fs")]
    mount: Option<Vec<(String, String)>>,

    /// Configure runtime preopen mappings
    #[arg(long, value_name("preopen=hostpreopen"), value_parser = parse_key_val::<String, String>, help_heading = "Fs")]
    preopen: Option<Vec<(String, String)>>,

    // STDIO
    /// Enable all stdio
    #[arg(long, default_missing_value="true", num_args=0..=1, help_heading = "Stdio")]
    allow_stdio: Option<bool>,
    /// Configure all stdio
    #[arg(long, value_enum, value_name("cfg"), num_args(0..=1), require_equals(true), default_missing_value("allow"), help_heading = "Stdio")]
    stdio: Option<StdioCfg>,
    /// Configure stderr
    #[arg(long, value_enum, value_name("cfg"), num_args(0..=1), require_equals(true), default_missing_value("allow"), help_heading = "Stdio")]
    stderr: Option<StdioCfg>,
    /// Configure stdin
    #[arg(long, value_enum, value_name("cfg"), num_args(0..=1), require_equals(true), default_missing_value("allow"), help_heading = "Stdio")]
    stdin: Option<StdioCfg>,
    /// Configure stdout
    #[arg(long, value_enum, value_name("cfg"), num_args(0..=1), require_equals(true), default_missing_value("allow"), help_heading = "Stdio")]
    stdout: Option<StdioCfg>,

    /// WASI version to use when creating performing stubbing (ex. when creating a new stub)
    #[arg(long)]
    wasi_version: Option<String>,
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

// Commas between KEY=VALUE pairs still separate overrides, as they always
// did; anywhere else they belong to the value.
fn expand_comma_pairs(pairs: Vec<(String, String)>) -> Vec<(String, String)> {
    pairs
        .into_iter()
        .flat_map(|(key, value)| {
            let arg = format!("{key}={value}");
            match arg
                .split(',')
                .map(parse_key_val::<String, String>)
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(parsed) if parsed.len() > 1 => parsed,
                _ => vec![(key, value)],
            }
        })
        .collect()
}

fn main() -> Result<()> {
    let args = Args::parse();

    let mut virt_opts = WasiVirt::default();

    virt_opts.debug(args.debug.unwrap_or_default());

    // By default, we virtualize all subsystems
    // This ensures full encapsulation in the default (no argument) case
    let allow_all = args.allow_all;
    let allow_stdio = args.allow_stdio.unwrap_or(allow_all);
    let stdio = if allow_stdio {
        StdioCfg::Allow
    } else {
        StdioCfg::Deny
    };

    // clocks
    virt_opts.clocks(args.allow_clocks.unwrap_or(allow_all));

    // http
    virt_opts.http(args.allow_http.unwrap_or(allow_all));

    // random
    virt_opts.random(args.allow_random.unwrap_or(allow_all));

    // sockets
    virt_opts.sockets(args.allow_sockets.unwrap_or(allow_all));

    // stdio
    virt_opts.stdio().stdin(args.stdin.unwrap_or(stdio.clone()));
    virt_opts
        .stdio()
        .stdout(args.stdout.unwrap_or(stdio.clone()));
    let stderr = args.stderr.unwrap_or(stdio.clone());

    if virt_opts.debug_enabled() && !matches!(stderr, StdioCfg::Allow) {
        bail!("Debug build requires stderr to be enabled");
    }
    virt_opts.stdio().stderr(stderr);

    // exit
    virt_opts.exit(args.allow_exit.unwrap_or(allow_all));

    // env options
    let env = virt_opts.env();
    match args.allow_env {
        Some(allow_env) if allow_env.len() == 0 => {
            env.allow_all();
        }
        Some(allow_env) => {
            env.allow(&allow_env);
        }
        None => {
            if allow_all {
                env.allow_all();
            }
        }
    };
    if let Some(env_overrides) = args.env {
        env.overrides = expand_comma_pairs(env_overrides);
    }

    // config options
    let config = virt_opts.config();
    match args.allow_config {
        Some(allow_config) if allow_config.len() == 0 => {
            config.allow_all();
        }
        Some(allow_config) => {
            config.allow(&allow_config);
        }
        None => {
            if allow_all {
                // TODO: enable once wasi:config/store is stable
                // config.allow_all();
            }
        }
    };
    if let Some(config_overrides) = args.config {
        config.overrides = expand_comma_pairs(config_overrides);
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
    if args.allow_fs.unwrap_or(allow_all) {
        fs.allow_host_preopens();
    }

    if let Some(compose) = args.compose {
        virt_opts.compose_component_path(compose);
        virt_opts.filter_imports()?;
    }

    virt_opts.wasi_version(
        args.wasi_version
            .as_ref()
            .and_then(|s| Version::parse(s).ok())
            .unwrap_or(DEFAULT_INSERT_WASI_VERSION),
    );

    let virt_component = virt_opts.finish()?;

    let out_path = PathBuf::from(args.out);
    if virt_component.virtual_files.len() > 0 {
        eprintln!("Virtualized files from local filesystem:\n");
        for (virtual_path, original_path) in virt_component.virtual_files {
            eprintln!("  - {virtual_path} : {original_path}");
        }
    }

    fs::write(&out_path, virt_component.adapter)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Args {
        Args::try_parse_from(args.iter().copied()).unwrap()
    }

    fn pair(key: &str, value: &str) -> (String, String) {
        (key.to_string(), value.to_string())
    }

    #[test]
    fn override_values_keep_their_commas() {
        let args = parse(&[
            "wasi-virt",
            "-o",
            "virt.wasm",
            "-e",
            r#"JSON=[{"a":1},{"b":2}]"#,
            "-e",
            "TEXT=Hello, world",
            "-c",
            "list=a,b",
            "--allow-env=HOST,PATH",
        ]);
        assert_eq!(
            expand_comma_pairs(args.env.unwrap()),
            [
                pair("JSON", r#"[{"a":1},{"b":2}]"#),
                pair("TEXT", "Hello, world")
            ]
        );
        assert_eq!(
            expand_comma_pairs(args.config.unwrap()),
            [pair("list", "a,b")]
        );
        // A list of names stays comma-separated.
        assert_eq!(args.allow_env.unwrap(), ["HOST", "PATH"]);
    }

    #[test]
    fn comma_separated_pairs_set_several_overrides() {
        let args = parse(&[
            "wasi-virt",
            "-o",
            "virt.wasm",
            "-e",
            "A=1,B=2",
            "-c",
            "x=y,z=w",
        ]);
        assert_eq!(
            expand_comma_pairs(args.env.unwrap()),
            [pair("A", "1"), pair("B", "2")]
        );
        assert_eq!(
            expand_comma_pairs(args.config.unwrap()),
            [pair("x", "y"), pair("z", "w")]
        );
    }

    #[test]
    fn existing_override_forms_still_parse() {
        let args = parse(&[
            "wasi-virt",
            "-o",
            "virt.wasm",
            "--env",
            "A=1",
            "--env",
            "B=2",
            "--config",
            "x=y",
            "--allow-env=HOST,PATH",
            "--allow-config=one,two",
        ]);
        assert_eq!(
            expand_comma_pairs(args.env.unwrap()),
            [pair("A", "1"), pair("B", "2")]
        );
        assert_eq!(expand_comma_pairs(args.config.unwrap()), [pair("x", "y")]);
        assert_eq!(args.allow_env.unwrap(), ["HOST", "PATH"]);
        assert_eq!(args.allow_config.unwrap(), ["one", "two"]);
    }
}
