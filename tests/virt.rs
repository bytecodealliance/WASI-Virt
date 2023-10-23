use anyhow::{anyhow, Context, Result};
use cap_std::ambient_authority;
use heck::ToSnakeCase;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::process::Command;
use std::{fs, path::PathBuf};
use wasi_virt::WasiVirt;
use wasm_compose::composer::ComponentComposer;
use wasmparser::{Chunk, Parser, Payload};
use wasmtime::{
    component::{Component, Linker},
    Config, Engine, Store, WasmBacktraceDetails,
};
use wasmtime_wasi::preview2::command::add_to_linker;
use wasmtime_wasi::preview2::{DirPerms, FilePerms, Table, WasiCtx, WasiCtxBuilder, WasiView};
use wasmtime_wasi::Dir;
use wit_component::ComponentEncoder;

wasmtime::component::bindgen!({
    world: "virt-test",
    path: "wit",
    async: true
});

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

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct TestExpectation {
    env: Option<Vec<(String, String)>>,
    file_read: Option<String>,
    encapsulation: Option<bool>,
    stdout: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct TestCase {
    component: String,
    host_env: Option<BTreeMap<String, String>>,
    host_fs_path: Option<String>,
    virt_opts: Option<WasiVirt>,
    expect: TestExpectation,
}

const DEBUG: bool = false;

#[tokio::test]
async fn virt_test() -> Result<()> {
    let wasi_adapter = fs::read("lib/wasi_snapshot_preview1.reactor.wasm")?;

    for test_case in fs::read_dir("tests/cases")? {
        let test_case = test_case?;
        let test_case_path = test_case.path();
        let test_case_file_name = test_case.file_name().to_string_lossy().to_string();
        let test_case_name = test_case_file_name.strip_suffix(".toml").unwrap();

        // Filtering...
        // if test_case_name != "stdio" {
        //     continue;
        // }

        println!("> {:?}", test_case_path);

        // load the test case JSON data
        let test: TestCase = toml::from_str(&fs::read_to_string(&test_case_path)?)
            .context(format!("Error reading test case {:?}", test_case_path))?;

        let component_name = &test.component;

        // build the generated test component
        let generated_path = PathBuf::from("tests/generated");
        fs::create_dir_all(&generated_path)?;

        if DEBUG {
            println!("- Building test component");
        }

        let mut generated_component_path = generated_path.join(component_name);
        generated_component_path.set_extension("component.wasm");
        cmd(&format!(
            "cargo build -p {component_name} --target wasm32-wasi {}",
            if DEBUG { "" } else { "--release" }
        ))?;

        if DEBUG {
            println!("- Encoding test component");
        }

        // encode the component
        let component_core = fs::read(&format!(
            "target/wasm32-wasi/{}/{}.wasm",
            if DEBUG { "debug" } else { "release" },
            component_name.to_snake_case()
        ))?;
        let mut encoder = ComponentEncoder::default()
            .validate(true)
            .module(&component_core)?;
        encoder = encoder.adapter("wasi_snapshot_preview1", wasi_adapter.as_slice())?;
        fs::write(
            &generated_component_path,
            encoder.encode().with_context(|| "Encoding component")?,
        )?;

        // create the test case specific virtualization
        if DEBUG {
            println!("- Creating virtualization");
        }

        let mut virt_component_path = generated_path.join(test_case_name);
        virt_component_path.set_extension("virt.wasm");
        let mut virt_opts = test.virt_opts.clone().unwrap_or_default();
        virt_opts.exit(Default::default());
        if DEBUG {
            virt_opts.debug = true;
            if test_case_name != "encapsulate" {
                virt_opts.wasm_opt = Some(false);
            }
        }

        let virt_component = virt_opts
            .finish()
            .with_context(|| format!("Error creating virtual adapter for {:?}", test_case_path))?;

        fs::write(&virt_component_path, &virt_component.adapter)?;

        // verify the encapsulation
        if test.expect.encapsulation.unwrap_or(false) {
            if let Some(impt) = has_component_import(virt_component.adapter.as_slice())? {
                panic!(
                    "Unexpected import \"{impt}\" in virtualization {:?}",
                    virt_component_path
                );
            }
        }

        // compose the test component with the defined test virtualization
        if DEBUG {
            println!("- Composing virtualization");
        }
        let component_bytes = ComponentComposer::new(
            &generated_component_path,
            &wasm_compose::config::Config {
                definitions: vec![virt_component_path],
                ..Default::default()
            },
        )
        .compose()?;

        if true {
            let mut composed_path = generated_path.join(test_case_name);
            composed_path.set_extension("composed.wasm");
            fs::write(composed_path, &component_bytes)?;
        }

        // execute the composed virtualized component test function
        if DEBUG {
            println!("- Executing composition");
        }
        let mut builder = WasiCtxBuilder::new();
        builder.inherit_stdio().preopened_dir(
            Dir::open_ambient_dir(".", ambient_authority())?,
            DirPerms::READ,
            FilePerms::READ,
            "/",
        );
        if let Some(host_env) = &test.host_env {
            for (k, v) in host_env {
                builder.env(k, v);
            }
        }
        let mut table = Table::new();
        let wasi = builder.build(&mut table)?;

        let mut config = Config::new();
        config.cache_config_load_default().unwrap();
        config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
        config.wasm_component_model(true);
        config.async_support(true);

        let engine = Engine::new(&config)?;
        let mut linker = Linker::new(&engine);

        let component = Component::from_binary(&engine, &component_bytes).unwrap();

        struct CommandCtx {
            table: Table,
            wasi: WasiCtx,
        }
        impl WasiView for CommandCtx {
            fn table(&self) -> &Table {
                &self.table
            }
            fn table_mut(&mut self) -> &mut Table {
                &mut self.table
            }
            fn ctx(&self) -> &WasiCtx {
                &self.wasi
            }
            fn ctx_mut(&mut self) -> &mut WasiCtx {
                &mut self.wasi
            }
        }

        add_to_linker(&mut linker)?;
        let mut store = Store::new(&engine, CommandCtx { table, wasi });

        let (instance, _instance) =
            VirtTest::instantiate_async(&mut store, &component, &linker).await?;

        if DEBUG {
            println!("- Checking expectations");
        }

        // env var expectation check
        if let Some(expect_env) = &test.expect.env {
            let env_vars = instance.call_test_get_env(&mut store).await?;
            if !env_vars.eq(expect_env) {
                return Err(anyhow!(
                    "Unexpected env vars testing {:?}:

    \x1b[1mExpected:\x1b[0m {:?}
    \x1b[1mActual:\x1b[0m {:?}

    {:?}",
                    test_case_path,
                    expect_env,
                    env_vars,
                    test
                ));
            }
        }

        // fs read expectation check
        if let Some(expect_file_read) = &test.expect.file_read {
            let file_read = instance
                .call_test_file_read(&mut store, test.host_fs_path.as_ref().unwrap())
                .await?;
            if file_read.starts_with("ERR") {
                eprintln!("> {}", file_read);
            }
            if !file_read.eq(expect_file_read) {
                return Err(anyhow!(
                    "Unexpected file read result testing {:?}",
                    test_case_path
                ));
            }
        }

        if let Some(_expect_stdout) = &test.expect.stdout {
            // todo: expectation pending wasmtime stream flushing
            instance.call_test_stdio(&mut store).await?;
        }

        println!("\x1b[1;32m√\x1b[0m {:?}", test_case_path);
    }
    Ok(())
}

fn has_component_import(bytes: &[u8]) -> Result<Option<String>> {
    let mut parser = Parser::new(0);
    let mut offset = 0;
    loop {
        let payload = match parser.parse(&bytes[offset..], true)? {
            Chunk::NeedMoreData(_) => unreachable!(),
            Chunk::Parsed { payload, consumed } => {
                offset += consumed;
                payload
            }
        };
        match payload {
            Payload::ModuleSection { mut parser, range } => {
                let mut ioffset = range.start;
                loop {
                    let payload = match parser.parse(&bytes[ioffset..], true)? {
                        Chunk::NeedMoreData(_) => unreachable!(),
                        Chunk::Parsed { payload, consumed } => {
                            ioffset += consumed;
                            payload
                        }
                    };
                    match payload {
                        Payload::ImportSection(impt_section_reader) => {
                            for impt in impt_section_reader {
                                let impt = impt?;
                                return Ok(Some(format!("{}#{}", impt.module, impt.name)));
                            }
                        }
                        Payload::End(_) => return Ok(None),
                        _ => {}
                    }
                }
            }
            Payload::End(_) => return Ok(None),
            _ => {}
        }
    }
}
