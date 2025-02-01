use std::sync::Arc;

use anyhow::{bail, Context, Result};
use component2json::{component_exports_to_json_schema, json_to_vals, vals_to_json};
use manager::LifecycleManager;
use mcp_sdk::server::Server;
use mcp_sdk::transport::{JsonRpcNotification, ServerStdioTransport, Transport};
use mcp_sdk::types::{
    CallToolRequest, CallToolResponse, ListRequest, ResourcesListResponse, ServerCapabilities,
    ToolDefinition, ToolResponseContent, ToolsListResponse,
};
use serde_json::{json, Value};
use tokio::task::block_in_place;
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiView};

mod manager;

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
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        // needs to be stderr due to stdio transport
        .with_writer(std::io::stderr)
        .init();

    let mut config = Config::new();
    config.wasm_component_model(true);
    config.async_support(true);
    let engine = Arc::new(Engine::new(&config)?);
    let (tools_changed_sender, mut tools_changed_receiver) = tokio::sync::mpsc::unbounded_channel();
    let manager = Arc::new(manager::LifecycleManager::new(
        engine.clone(),
        tools_changed_sender,
    ));

    // manager.load_component("filesystem", "/Users/mossaka/Developer/mossaka/mcp-wasmtime/examples/filesystem2/filesystem2.wasm").await?;

    let protocol = ServerStdioTransport;

    let server = Server::builder(protocol.clone())
        .capabilities(ServerCapabilities {
            tools: Some(json!({"listChanged": true})),
            ..Default::default()
        })
        .request_handler("tools/list", {
            let manager_clone = manager.clone();
            move |req: ListRequest| -> Result<ToolsListResponse> {
                block_in_place(|| {
                    tokio::runtime::Handle::current()
                        .block_on(list_tools(req, manager_clone.clone()))
                })
            }
        })
        .request_handler("tools/call", {
            let manager_clone = manager.clone();
            move |req: CallToolRequest| -> Result<CallToolResponse> {
                block_in_place(|| {
                    tokio::runtime::Handle::current()
                        .block_on(call_tool(req, manager_clone.clone()))
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

    let notification_task = tokio::spawn(async move {
        while let Some(()) = tools_changed_receiver.recv().await {
            tracing::info!("Tools changed, sending notification");
            let notification = JsonRpcNotification {
                method: "notifications/tools/list_changed".to_string(),
                ..Default::default()
            };
            let msg = mcp_sdk::transport::JsonRpcMessage::Notification(notification);
            let msg_string = serde_json::to_string(&msg).expect("Failed to serialize JSON");
            tracing::info!("Sending notification: {}", msg_string);
            protocol.send(&msg).expect("Failed to send notification");
        }
    });

    tokio::select! {
        res = server_handle => {
            res??;
        }
        _ = notification_task => {}
    }
    Ok(())
}

async fn new_store_and_instance(
    engine: Arc<Engine>,
    component: Arc<Component>,
) -> Result<(Store<MyWasi>, wasmtime::component::Instance)> {
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
    Ok((store, instance))
}

pub async fn call_tool(
    req: CallToolRequest,
    manager: Arc<LifecycleManager>,
) -> Result<CallToolResponse> {
    match req.name.as_str() {
        "load-component" => {
            let args = req.arguments.unwrap_or(Value::Null);
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'id' in arguments for load-component"))?;
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'path' in arguments for load-component"))?;

            manager.load_component(id, path).await?;

            let reply = json!({
                "status": "component loaded",
                "id": id
            });
            Ok(CallToolResponse {
                content: vec![ToolResponseContent::Text {
                    text: reply.to_string(),
                }],
                is_error: None,
                meta: None,
            })
        }
        "unload-component" => {
            let args = req.arguments.unwrap_or(Value::Null);
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'id' in arguments for unload-component"))?;

            manager.unload_component(id).await?;

            let reply = json!({
                "status": "component unloaded",
                "id": id
            });
            Ok(CallToolResponse {
                content: vec![ToolResponseContent::Text {
                    text: reply.to_string(),
                }],
                is_error: None,
                meta: None,
            })
        }
        _ => {
            let arguments_json = req.arguments.clone().unwrap_or(Value::Null);
            let (component_id_opt, new_arguments) = if let Some(obj) = arguments_json.as_object() {
                let mut obj_clone = obj.clone();
                let comp_id = obj_clone
                    .remove("componentId")
                    .and_then(|v| v.as_str().map(|s| s.to_string()));
                (comp_id, Value::Object(obj_clone))
            } else {
                (None, req.arguments.unwrap_or(Value::Null))
            };
            let component = manager.get_component(component_id_opt.as_deref()).await?;
            let (mut store, instance) =
                new_store_and_instance(manager.engine.clone(), component.clone()).await?;

            let schema = component_exports_to_json_schema(&component, store.engine(), true);
            let empty_tools = vec![];
            let tools_array = schema["tools"].as_array().unwrap_or(&empty_tools);
            let maybe_tool = tools_array
                .iter()
                .find(|tool_json| tool_json["name"].as_str() == Some(&req.name));

            let tool = match maybe_tool {
                Some(t) => t,
                None => bail!("No exported function named '{}'", req.name),
            };

            tracing::info!("Calling tool '{}'", req.name);

            let arguments_json = new_arguments;
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
    }
}

pub async fn list_tools(
    _req: ListRequest,
    manager: Arc<LifecycleManager>,
) -> Result<ToolsListResponse> {
    let mut tools = Vec::new();
    {
        let comps = manager.components.read().await;
        for (comp_id, component) in comps.iter() {
            let schema =
                component_exports_to_json_schema(component, manager.engine.as_ref(), false);
            if let Some(arr) = schema.get("tools").and_then(|v| v.as_array()) {
                for tool_json in arr {
                    let mut tool = tool_json.clone();
                    if let Some(obj) = tool.as_object_mut() {
                        obj.insert("componentId".to_string(), json!(comp_id));
                    }
                    let name = tool
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("<unnamed>")
                        .to_string();
                    let description = tool
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let input_schema = tool.get("inputSchema").cloned().unwrap_or(json!({}));
                    tools.push(ToolDefinition {
                        name,
                        description,
                        input_schema,
                    });
                }
            }
        }
    }
    // Add management tools for component lifecycle actions.
    tools.push(ToolDefinition {
        name: "load-component".to_string(),
        description: Some(
            "Dynamically loads a new WebAssembly component. Arguments: id (string), path (string)"
                .to_string(),
        ),
        input_schema: json!({
            "type": "object",
            "properties": {
                "id": {"type": "string"},
                "path": {"type": "string"}
            },
            "required": ["id", "path"]
        }),
    });
    tools.push(ToolDefinition {
        name: "unload-component".to_string(),
        description: Some(
            "Dynamically unloads a WebAssembly component. Argument: id (string)".to_string(),
        ),
        input_schema: json!({
            "type": "object",
            "properties": {
                "id": {"type": "string"}
            },
            "required": ["id"]
        }),
    });
    Ok(ToolsListResponse {
        tools,
        next_cursor: None,
        meta: None,
    })
}
