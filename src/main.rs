use std::sync::Arc;
use tonic::transport::Channel;
use tokio::task::block_in_place;
use mcp_sdk::transport::{JsonRpcNotification, ServerStdioTransport, Transport};
use mcp_sdk::server::Server;
use mcp_sdk::types::{
    CallToolRequest, CallToolResponse, ListRequest, ToolsListResponse, 
    ToolResponseContent, ToolDefinition, ServerCapabilities,
};
use serde_json::{json, Value};
use anyhow::Result;
use wasmtime::component::{Component, Linker};
use wasmtime::{Store, Engine};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiView};
use component2json::{json_to_vals, vals_to_json};

pub mod lifecycle {
    tonic::include_proto!("lifecycle");
}

use lifecycle::{
    lifecycle_manager_service_client::LifecycleManagerServiceClient,
    GetComponentRequest, LoadComponentRequest, UnloadComponentRequest,
    ListComponentsRequest,
};

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

pub struct Client {
    transport: ServerStdioTransport,
    grpc_client: Arc<tokio::sync::Mutex<LifecycleManagerServiceClient<Channel>>>,
}

impl Client {
    pub async fn new(grpc_addr: String) -> Result<Self, Box<dyn std::error::Error>> {
        let grpc_client = LifecycleManagerServiceClient::connect(format!("http://{}", grpc_addr)).await?;
        Ok(Self {
            transport: ServerStdioTransport,
            grpc_client: Arc::new(tokio::sync::Mutex::new(grpc_client)),
        })
    }

    pub async fn serve(self) -> Result<(), Box<dyn std::error::Error>> {
        let server = Server::builder(self.transport.clone())
            .capabilities(ServerCapabilities {
                tools: Some(json!({"listChanged": true})),
                ..Default::default()
            })
            .request_handler("tools/list", {
                let grpc_client = self.grpc_client.clone();
                move |_req: ListRequest| -> Result<ToolsListResponse> {
                    block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(async {
                            let mut client = grpc_client.lock().await;
                            let components = client.list_components(ListComponentsRequest {}).await?.into_inner();
                            let mut tools = Vec::new();

                            for id in components.ids {
                                let response = client.get_component(GetComponentRequest {
                                    id: id.clone(),
                                }).await?;
                                let schema: Value = serde_json::from_str(&response.into_inner().details)?;
                                
                                if let Some(arr) = schema.get("tools").and_then(|v| v.as_array()) {
                                    for tool_json in arr {
                                        let name = tool_json.get("name")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("<unnamed>")
                                            .to_string();
                                        let description = tool_json.get("description")
                                            .and_then(|v| v.as_str())
                                            .map(|s| s.to_string());
                                        let input_schema = tool_json.get("inputSchema")
                                            .cloned()
                                            .unwrap_or(json!({}));
                                        tools.push(ToolDefinition {
                                            name,
                                            description,
                                            input_schema,
                                        });
                                    }
                                }
                            }

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
                        })
                    })
                }
            })
            .request_handler("tools/call", {
                let grpc_client = self.grpc_client.clone();
                move |req: CallToolRequest| -> Result<CallToolResponse> {
                    block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(async {
                            let mut client = grpc_client.lock().await;
                            
                            match req.name.as_str() {
                                "load-component" => {
                                    let args = req.arguments.unwrap_or(Value::Null);
                                    let id = args.get("id")
                                        .and_then(|v| v.as_str())
                                        .ok_or_else(|| anyhow::anyhow!("Missing 'id' in arguments"))?;
                                    let path = args.get("path")
                                        .and_then(|v| v.as_str())
                                        .ok_or_else(|| anyhow::anyhow!("Missing 'path' in arguments"))?;

                                    let response = client.load_component(LoadComponentRequest {
                                        id: id.to_string(),
                                        path: path.to_string(),
                                    }).await?;

                                    Ok(CallToolResponse {
                                        content: vec![ToolResponseContent::Text {
                                            text: serde_json::to_string(&json!({
                                                "status": "component loaded",
                                                "id": id
                                            }))?,
                                        }],
                                        is_error: None,
                                        meta: None,
                                    })
                                }
                                "unload-component" => {
                                    let args = req.arguments.unwrap_or(Value::Null);
                                    let id = args.get("id")
                                        .and_then(|v| v.as_str())
                                        .ok_or_else(|| anyhow::anyhow!("Missing 'id' in arguments"))?;

                                    let response = client.unload_component(UnloadComponentRequest {
                                        id: id.to_string(),
                                    }).await?;

                                    Ok(CallToolResponse {
                                        content: vec![ToolResponseContent::Text {
                                            text: serde_json::to_string(&json!({
                                                "status": "component unloaded",
                                                "id": id
                                            }))?,
                                        }],
                                        is_error: None,
                                        meta: None,
                                    })
                                }
                                _ => {
                                    let arguments_json = req.arguments.clone().unwrap_or(Value::Null);
                                    let (component_id_opt, new_arguments) = if let Some(obj) = arguments_json.as_object() {
                                        let mut obj_clone = obj.clone();
                                        let comp_id = obj_clone.remove("componentId")
                                            .and_then(|v| v.as_str().map(|s| s.to_string()));
                                        (comp_id, Value::Object(obj_clone))
                                    } else {
                                        (None, req.arguments.unwrap_or(Value::Null))
                                    };

                                    let component_response = client.get_component(GetComponentRequest {
                                        id: component_id_opt.unwrap_or_default(),
                                    }).await?.into_inner();

                                    let schema: Value = serde_json::from_str(&component_response.details)?;
                                    let tools = schema.get("tools")
                                        .and_then(|v| v.as_array())
                                        .ok_or_else(|| anyhow::anyhow!("No tools found in component"))?;

                                    let tool = tools.iter()
                                        .find(|t| t.get("name").and_then(|n| n.as_str()) == Some(&req.name))
                                        .ok_or_else(|| anyhow::anyhow!("Tool not found"))?;

                                    let mut config = wasmtime::Config::new();
                                    config.wasm_component_model(true);
                                    config.async_support(true);
                                    let engine = Arc::new(Engine::new(&config)?);

                                    // Get the component from the response
                                    let component = Arc::new(Component::new(
                                        engine.as_ref(),
                                        &std::fs::read(&component_response.id)?
                                    )?);

                                    let (mut store, instance) = new_store_and_instance(
                                        engine,
                                        component
                                    ).await?;

                                    let argument_vals = json_to_vals(&new_arguments)?;
                                    let output_schema = tool["outputSchema"].clone();
                                    let mut results = json_to_vals(&output_schema)?;

                                    let export_index = instance
                                        .get_export(&mut store, None, &req.name)
                                        .ok_or_else(|| anyhow::anyhow!("Failed to get export '{}'", &req.name))?;

                                    let func = instance
                                        .get_func(&mut store, &export_index)
                                        .ok_or_else(|| anyhow::anyhow!("Failed to get function"))?;

                                    func.call_async(&mut store, &argument_vals, &mut results).await?;
                                    let results = serde_json::to_string_pretty(&vals_to_json(&results))?;

                                    Ok(CallToolResponse {
                                        content: vec![ToolResponseContent::Text {
                                            text: results,
                                        }],
                                        is_error: None,
                                        meta: None,
                                    })
                                }
                            }
                        })
                    })
                }
            })
            .build();

        // TODO: this part needs a redesign to figure out how to send notifcations
        // let notification_task = {
        //     let transport = self.transport.clone();
        //     tokio::spawn(async move {
        //         while let Some(()) = tools_changed_receiver.recv().await {
        //             tracing::info!("Tools changed, sending notification");
        //             let notification = JsonRpcNotification {
        //                 method: "notifications/tools/list_changed".to_string(),
        //                 ..Default::default()
        //             };
        //             let msg = mcp_sdk::transport::JsonRpcMessage::Notification(notification);
        //             if let Ok(msg_string) = serde_json::to_string(&msg) {
        //                 tracing::info!("Sending notification: {}", msg_string);
        //                 let _ = transport.send(&msg);
        //             }
        //         }
        //     })
        // };

        tokio::select! {
            res = server.listen() => { res?; }
            _ = notification_task => {}
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(std::io::stderr)
        .init();
    
    let client = Client::new("[::1]:50051".to_string()).await?;
    client.serve().await
}
