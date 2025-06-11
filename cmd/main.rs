use std::sync::Arc;

use anyhow::{bail, Context, Result};
use component2json::{json_to_vals, vals_to_json};
use wasmtime::component::{Component, Linker, Val};
use wasmtime::{Config, Engine, Store};

use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiView};

struct MyWasi {
    ctx: WasiCtx,
    table: wasmtime_wasi::ResourceTable,
}

impl WasiView for MyWasi {
    fn table(&mut self) -> &mut wasmtime_wasi::ResourceTable {
        &mut self.table
    }

    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.ctx
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.async_support(true);
    let engine = Arc::new(Engine::new(&config)?);

    let component = Arc::new(Component::from_file(
        &engine,
        "examples/filesystem/target/wasm32-wasip2/release/filesystem.wasm",
    )?);

    let schema = component2json::component_exports_to_json_schema(&component, &engine, true);
    if let Some(arr) = schema["tools"].as_array() {
        for t in arr {
            let name = t["name"].as_str().unwrap_or("<unnamed>").to_string();
            let description: Option<String> = t["description"].as_str().map(|s| s.to_string());
            let input_schema = t["inputSchema"].clone(); // already a serde_json::Value

            println!("{}, {:?}", name, description);
        }
    }
    let mut linker = Linker::new(&engine);
    let _ = wasmtime_wasi::add_to_linker_async(&mut linker);

    let mut store = Store::new(
        &engine,
        MyWasi {
            ctx: WasiCtx::builder()
                .inherit_args()
                .inherit_env()
                .inherit_stdio()
                .preopened_dir("/", "/", DirPerms::READ, FilePerms::READ)?
                .build(),
            table: wasmtime_wasi::ResourceTable::default(),
        },
    );
    let instance = linker.instantiate_async(&mut store, &component).await?;
    let empty_tools = vec![];
    let tools_array = schema["tools"].as_array().unwrap_or(&empty_tools);
    let maybe_tool = tools_array
        .iter()
        .find(|tool_json| tool_json["name"].as_str() == Some("list-directory"));

    let tool = match maybe_tool {
        Some(t) => t,
        None => bail!("No exported function named '{}'", "list-directory"),
    };
    let argument_val = vec![Val::String(".".to_string())];
    let mut result_string = String::new();
    let export_index = instance.get_export(&mut store, None, "list-directory").context(format!(
        "Failed to get export '{}'",
        "list-directory",
    ))?;

    let func = instance
        .get_func(&mut store, &export_index)
        .context("Failed to get function")?;
    
    let outputSchema = tool["outputSchema"].clone();
    let mut results = json_to_vals(&outputSchema)?;
    println!("Results: {:?}", results);
    func.call_async(&mut store, &argument_val, &mut results).await?;
    // for result in results {
    //     result_string.push_str(&format!("{:?}", result));
    // }
    println!("Results: {}", serde_json::to_string_pretty(&vals_to_json(&results))?);


    // println!("Results: {:?}", result_string);
    Ok(())
}
