use std::sync::Arc;

use anyhow::{Context, Result};
use mcp_sdk::server::Server;
use mcp_sdk::transport::ServerStdioTransport;
use mcp_sdk::types::{
    CallToolRequest, CallToolResponse, ListRequest, ResourcesListResponse, ServerCapabilities,
    ToolDefinition, ToolResponseContent, ToolsListResponse,
};
use mossaka::mcp::types;
use serde_json::json;
use tokio::task::block_in_place;
use wasmtime::component::{bindgen, Component, Linker};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiView};

bindgen!({
    world: "mcp",
    path: "mcp.wit",
    async: true,
});

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
        "/Users/mossaka/Developer/mossaka/mcp-wasmtime/examples/filesystem/filesystem.wasm",
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
                    tokio::runtime::Handle::current()
                        .block_on(list_tools(req, wasm_env.clone()))
                })
            }
        })
        .request_handler("tools/call", {
            let wasm_env = wasm_env.clone();
            move |req: CallToolRequest| -> Result<CallToolResponse> {
                block_in_place(|| {
                    // Synchronously block on the async call
                    tokio::runtime::Handle::current()
                        .block_on(call_tool(req, wasm_env.clone()))
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

async fn new_store_and_instance(env: &WasmEnv) -> Result<(Store<MyWasi>, wasmtime::component::Instance)> {
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
    Ok((store, instance))
}

pub async  fn call_tool(req: CallToolRequest, env: WasmEnv) -> Result<CallToolResponse> {
    let (mut store, instance) = new_store_and_instance(&env).await?;

    let func = instance
        .get_export(&mut store, None, "mossaka:mcp/tool-server@0.1.0")
        .and_then(|i| instance.get_export(&mut store, Some(&i), "call-tool"))
        .context("missing the expected 'call-tool' function")?;
    let call_tool_fn = instance
        .get_typed_func::<(types::CallToolRequest,), (types::CallToolResponse,)>(
            &mut store, &func,
        )?;

    let wit_req = types::CallToolRequest {
        name: req.name,
        arguments: req.arguments
            .map(|v| serde_json::to_string(&v))
            .transpose()?, // Converts Option<Result<_,_>> to Result<Option<_>,_>
        meta: req.meta
            .map(|v| serde_json::to_string(&v))
            .transpose()?,
    };

    let wit_resp = call_tool_fn.call_async(&mut store, (wit_req,)).await?;
    let (response,) = wit_resp;

    let content = response
        .content
        .into_iter()
        .map(|c| match c {
            types::ToolResponseContent::Text(t) => ToolResponseContent::Text { text: t.text },
        })
        .collect();

    Ok(CallToolResponse {
        content,
        is_error: response.is_error,
        meta: match response.meta {
            Some(m) => Some(serde_json::from_str(&m)?),
            None => None,
        },
    })
}

pub async fn list_tools(_req: ListRequest, env: WasmEnv) -> Result<ToolsListResponse> {
    let (mut store, instance) = new_store_and_instance(&env).await?;

    let func = instance
        .get_export(&mut store, None, "mossaka:mcp/tool-server@0.1.0")
        .and_then(|i| instance.get_export(&mut store, Some(&i), "list-tools"))
        .context("missing the expected 'list-tools' function")?;
    let call_tool_fn = instance
        .get_typed_func::<(types::ListToolsRequest,), (types::ListToolsResponse,)>(
            &mut store, &func,
        )?;
    let wit_req = types::ListToolsRequest {
        cursor: None,
        meta: None,
    };

    let (response,) = call_tool_fn.call_async(&mut store, (wit_req,)).await?;
    

    let mut tools = Vec::new();
    for t in response.tools {
        tools.push(ToolDefinition {
            name: t.name,
            description: t.description,
            input_schema: serde_json::from_str(&t.input_schema)?,
        });
    }

    Ok(ToolsListResponse {
        tools,
        next_cursor: response.next_cursor,
        meta: match response.meta {
            Some(m) => Some(serde_json::from_str(&m)?),
            None => None,
        },
    })
}
