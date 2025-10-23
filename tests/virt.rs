use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};
use heck::ToSnakeCase;
use log::debug;
use serde::Deserialize;
use wasi_virt::WasiVirt;
use wasm_compose::composer::ComponentComposer;
use wasmparser::{Chunk, Parser, Payload};
use wasmtime::component::ResourceTable;
use wasmtime::Cache;
use wasmtime::{
    component::{Component, Linker},
    Config, Engine, Store, WasmBacktraceDetails,
};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use wasmtime_wasi_config::{WasiConfig, WasiConfigVariables};
use wit_component::{ComponentEncoder, DecodedWasm};
use wit_parser::WorldItem;

wasmtime::component::bindgen!({
    world: "virt-test",
    path: "wit/0_2_1",
    exports: {
        default: async,
    },
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
    config: Option<Vec<(String, String)>>,
    file_read: Option<String>,
    encapsulation: Option<bool>,
    stdout: Option<String>,
    imports: Option<TestExpectationImports>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct TestExpectationImports {
    required: Option<Vec<String>>,
    disallowed: Option<Vec<String>>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct TestCase {
    component: String,
    compose: Option<bool>,
    host_env: Option<BTreeMap<String, String>>,
    host_config: Option<BTreeMap<String, String>>,
    host_fs_path: Option<String>,
    virt_opts: Option<WasiVirt>,
    expect: TestExpectation,
}

#[tokio::test]
async fn virt_test() -> Result<()> {
    let debug_enabled: bool = std::env::var("TEST_DEBUG").is_ok();
    if debug_enabled {
        env_logger::builder().is_test(true).try_init()?;
    }

    let wasi_adapter = fs::read("lib/wasi_snapshot_preview1.reactor.wasm")?;

    for test_case in fs::read_dir("tests/cases")? {
        let test_case = test_case?;
        let test_case_path = test_case.path();
        let test_case_file_name = test_case.file_name().to_string_lossy().to_string();
        let test_case_name = test_case_file_name.strip_suffix(".toml").unwrap();

        // Filtering...
        // if !test_case_name.starts_with("fs") {
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

        debug!("- Building test component");

        let mut generated_component_path = generated_path.join(component_name);
        generated_component_path.set_extension("component.wasm");
        cmd(&format!(
            "cargo build -p {component_name} --target wasm32-wasip1 {}",
            if debug_enabled { "" } else { "--release" }
        ))?;

        debug!("- Encoding test component");

        // encode the component
        let component_core_path = &format!(
            "target/wasm32-wasip1/{}/{}.wasm",
            if debug_enabled { "debug" } else { "release" },
            component_name.to_snake_case()
        );
        let component_core = fs::read(component_core_path)?;
        let mut encoder = ComponentEncoder::default()
            .validate(true)
            .module(&component_core)?;
        encoder = encoder.adapter("wasi_snapshot_preview1", wasi_adapter.as_slice())?;
        fs::write(
            &generated_component_path,
            encoder.encode().with_context(|| "Encoding component")?,
        )?;

        // create the test case specific virtualization
        debug!("- Creating virtualization");

        let mut virt_component_path = generated_path.join(test_case_name);
        virt_component_path.set_extension("wasm");
        let mut virt_opts = test.virt_opts.clone().unwrap_or_default();
        virt_opts.exit(Default::default());
        if debug_enabled {
            virt_opts.debug(true);
            if test_case_name != "encapsulate" {
                virt_opts.wasm_opt(false);
            }
        }
        if let Some(compose) = test.compose {
            if compose {
                let compose_path = generated_component_path
                    .clone()
                    .into_os_string()
                    .into_string()
                    .unwrap();
                virt_opts.compose_component_path(compose_path);
                virt_opts.filter_imports()?;
            }
        }

        // TODO: move to 0.2.3 in tests
        virt_opts.wasi_version(semver::Version::new(0, 2, 1));

        let virt_component = virt_opts.finish().with_context(|| {
            format!(
                "Error creating virtual adapter {:?} for {:?}",
                test_case_path, component_core_path
            )
        })?;

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

        let mut composed_path = generated_path.join(test_case_name);
        composed_path.set_extension("composed.wasm");
        let component_bytes = match test.compose {
            Some(true) => {
                // adapter is already composed
                virt_component.adapter
            }
            _ => {
                // compose the test component with the defined test virtualization
                debug!("- Composing virtualization");
                let component_bytes = ComponentComposer::new(
                    &generated_component_path,
                    &wasm_compose::config::Config {
                        definitions: vec![virt_component_path],
                        ..Default::default()
                    },
                )
                .compose()
                .context("failed to compose virtualization")?;

                fs::write(&composed_path, &component_bytes)?;

                component_bytes
            }
        };

        // execute the composed virtualized component test function
        debug!("- Executing composition");
        let mut builder = WasiCtxBuilder::new();
        let _ = builder
            .inherit_stdio()
            .preopened_dir(".", "/", DirPerms::READ, FilePerms::READ);
        if let Some(host_env) = &test.host_env {
            for (k, v) in host_env {
                builder.env(k, v);
            }
        }
        let wasi_config = {
            let mut config = WasiConfigVariables::new();
            if let Some(host_config) = &test.host_config {
                for (k, v) in host_config {
                    config.insert(k, v);
                }
            }
            config
        };
        let table = ResourceTable::new();
        let wasi = builder.build();

        let mut config = Config::new();
        config.cache(Some(Cache::from_file(None).unwrap()));
        config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
        config.wasm_component_model(true);
        config.async_support(true);

        let engine = Engine::new(&config)?;
        let mut linker = Linker::new(&engine);

        let component = Component::from_binary(&engine, &component_bytes).unwrap();

        struct CommandCtx {
            table: ResourceTable,
            wasi: WasiCtx,
            wasi_config: WasiConfigVariables,
        }
        impl WasiView for CommandCtx {
            fn ctx(&mut self) -> WasiCtxView<'_> {
                WasiCtxView {
                    ctx: &mut self.wasi,
                    table: &mut self.table,
                }
            }
        }
        impl CommandCtx {
            fn wasi_config(&mut self) -> &mut WasiConfigVariables {
                &mut self.wasi_config
            }
        }

        wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
        wasmtime_wasi_config::add_to_linker(&mut linker, |ctx: &mut CommandCtx| {
            WasiConfig::new(ctx.wasi_config())
        })?;
        let mut store = Store::new(
            &engine,
            CommandCtx {
                table,
                wasi,
                wasi_config,
            },
        );

        let instance = VirtTest::instantiate_async(&mut store, &component, &linker).await?;

        debug!("- Checking expectations");

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

        // config property expectation check
        if let Some(expect_config) = &test.expect.config {
            let config_props = instance.call_test_get_config(&mut store).await?;
            if !config_props.eq(expect_config) {
                return Err(anyhow!(
                    "Unexpected config properties testing {:?}:

    \x1b[1mExpected:\x1b[0m {:?}
    \x1b[1mActual:\x1b[0m {:?}

    {:?}",
                    test_case_path,
                    expect_config,
                    config_props,
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
                debug!("> {}", file_read);
            }
            if !file_read.eq(expect_file_read) {
                debug!("expected: {expect_file_read}\n");
                debug!("got: {file_read}\n");
                bail!("Unexpected file read result testing [{test_case_path:?}]",);
            }
        }

        if let Some(_expect_stdout) = &test.expect.stdout {
            // todo: expectation pending wasmtime stream flushing
            instance.call_test_stdio(&mut store).await?;
        }

        if let Some(expect_imports) = &test.expect.imports {
            let component_imports = collect_component_imports(component_bytes)?;

            if let Some(required_imports) = &expect_imports.required {
                for required_import in required_imports {
                    if !component_imports
                        .iter()
                        .any(|i| i.starts_with(required_import))
                    {
                        return Err(anyhow!(
                            "Required import missing {required_import} {:?}",
                            test_case_path
                        ));
                    }
                }
            }
            if let Some(disallowed_imports) = &expect_imports.disallowed {
                for disallowed_import in disallowed_imports {
                    if component_imports
                        .iter()
                        .any(|i| i.starts_with(disallowed_import))
                    {
                        return Err(anyhow!(
                            "Disallowed import {disallowed_import} {:?}",
                            test_case_path
                        ));
                    }
                }
            }
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
            Payload::ModuleSection {
                mut parser,
                unchecked_range: range,
            } => {
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
                                if !impt.module.starts_with("[export]") {
                                    return Ok(Some(format!("{}#{}", impt.module, impt.name)));
                                }
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

fn collect_component_imports(component_bytes: Vec<u8>) -> Result<Vec<String>> {
    let (resolve, world_id) = match wit_component::decode(&component_bytes)? {
        DecodedWasm::WitPackage(..) => {
            bail!("expected a component, found a WIT package")
        }
        DecodedWasm::Component(resolve, world_id) => (resolve, world_id),
    };

    let mut import_ids: Vec<String> = vec![];
    for (_, import) in &resolve.worlds[world_id].imports {
        if let WorldItem::Interface { id, .. } = import {
            if let Some(id) = resolve.id_of(*id) {
                import_ids.push(id);
            }
        }
    }

    Ok(import_ids)
}
