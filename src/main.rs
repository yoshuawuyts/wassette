use std::sync::Arc;

use anyhow::{bail, Context, Result};
use component2json::{component_exports_to_json_schema, json_to_vals, vals_to_json};
use mcp_sdk::server::Server;
use mcp_sdk::transport::ServerStdioTransport;
use mcp_sdk::types::{
    CallToolRequest, CallToolResponse, ListRequest, ResourcesListResponse, ServerCapabilities,
    ToolDefinition, ToolResponseContent, ToolsListResponse,
};
use serde_json::{json, Value};
use tokio::task::block_in_place;
use wasmtime::component::{Component, Linker};
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

#[derive(Clone)]
pub struct WasmEnv {
    engine: Arc<Engine>,
    component: Arc<Component>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        // needs to be stderr due to stdio transport
        .with_writer(std::io::stderr)
        .init();

    let mut config = Config::new();
    config.wasm_component_model(true);
    config.async_support(true);
    let engine = Arc::new(Engine::new(&config)?);

    let component = Arc::new(Component::from_file(
        &engine,
        "/Users/mossaka/Developer/mossaka/mcp-wasmtime/examples/filesystem2/filesystem2.wasm",
    )?);

    let wasm_env = WasmEnv {
        engine: engine.clone(),
        component: component.clone(),
    };

    let server = Server::builder(ServerStdioTransport)
        .capabilities(ServerCapabilities {
            tools: Some(json!({})),
            ..Default::default()
        })
        .request_handler("tools/list", {
            let wasm_env = wasm_env.clone();
            move |req: ListRequest| -> Result<ToolsListResponse> {
                block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(list_tools(req, wasm_env.clone()))
                })
            }
        })
        .request_handler("tools/call", {
            let wasm_env = wasm_env.clone();
            move |req: CallToolRequest| -> Result<CallToolResponse> {
                block_in_place(|| {
                    // Synchronously block on the async call
                    tokio::runtime::Handle::current().block_on(call_tool(req, wasm_env.clone()))
                })
            }
        })
        .request_handler("resources/list", |_req: ListRequest| {
            Ok(ResourcesListResponse {
                resources: vec![],
                next_cursor: None,
                meta: None,
            })
        })
        .build();
    let server_handle = {
        let server = server;
        tokio::spawn(async move { server.listen().await })
    };

    server_handle
        .await?
        .map_err(|e| anyhow::anyhow!("Server error: {}", e))?;
    Ok(())
}

async fn new_store_and_instance(
    env: &WasmEnv,
) -> Result<(Store<MyWasi>, wasmtime::component::Instance, &Component)> {
    let mut linker = Linker::new(&env.engine);
    let _ = wasmtime_wasi::add_to_linker_async(&mut linker);

    let mut store = Store::new(
        &env.engine,
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
    let instance = linker.instantiate_async(&mut store, &env.component).await?;
    Ok((store, instance, &env.component))
}

pub async fn call_tool(req: CallToolRequest, env: WasmEnv) -> Result<CallToolResponse> {
    let (mut store, instance, component) = new_store_and_instance(&env).await?;
    // get the schema of the component
    let schema = component_exports_to_json_schema(&component, store.engine(), true);
    let empty_tools = vec![];
    let tools_array = schema["tools"].as_array().unwrap_or(&empty_tools);
    let maybe_tool = tools_array
        .iter()
        .find(|tool_json| tool_json["name"].as_str() == Some(&req.name));

    // get the tool schema
    let tool = match maybe_tool {
        Some(t) => t,
        None => bail!("No exported function named '{}'", req.name),
    };

    tracing::info!("Calling tool '{}'", req.name);

    let arguments_json = req.arguments.clone().unwrap_or(Value::Null);
    let argument_vals = component2json::json_to_vals(&arguments_json)
        .context("Failed to parse the function arguments into Val")?;

    let export_index = instance
        .get_export(&mut store, None, &req.name)
        .context(format!("Failed to get export '{}'", &req.name,))?;

    let func = instance
        .get_func(&mut store, &export_index)
        .context("Failed to get function")?;

    let output_schema = tool["outputSchema"].clone();
    let mut results = json_to_vals(&output_schema)?;
    func.call_async(&mut store, &argument_vals, &mut results)
        .await?;

    let results = serde_json::to_string_pretty(&vals_to_json(&results))?;
    Ok(CallToolResponse {
        content: vec![ToolResponseContent::Text { text: results }],
        is_error: None,
        meta: None,
    })
}

pub async fn list_tools(_req: ListRequest, env: WasmEnv) -> Result<ToolsListResponse> {
    let (store, _, component) = new_store_and_instance(&env).await?;

    let schema = component2json::component_exports_to_json_schema(component, store.engine(), false);
    let mut tools = Vec::new();
    if let Some(arr) = schema["tools"].as_array() {
        for t in arr {
            let name = t["name"].as_str().unwrap_or("<unnamed>").to_string();
            let description: Option<String> = t["description"].as_str().map(|s| s.to_string());
            let input_schema = t["inputSchema"].clone(); // already a serde_json::Value

            tools.push(ToolDefinition {
                name,
                description,
                input_schema,
            });
        }
    }

    Ok(ToolsListResponse {
        tools,
        next_cursor: None,
        meta: None,
    })
}
