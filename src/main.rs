use std::sync::Arc;
use tonic::transport::Channel;
use tokio::task::block_in_place;
use mcp_sdk::transport::ServerStdioTransport;
use mcp_sdk::server::Server;
use mcp_sdk::types::{
    CallToolRequest, CallToolResponse, ListRequest, ToolsListResponse, 
    ToolResponseContent, ToolDefinition, ServerCapabilities,
};
use serde_json::{json, Value};
use anyhow::Result;

pub mod lifecycle {
    tonic::include_proto!("lifecycle");
}

use lifecycle::{
    lifecycle_manager_service_client::LifecycleManagerServiceClient,
    GetComponentRequest, LoadComponentRequest, UnloadComponentRequest,
    ListComponentsRequest, CallComponentRequest,
};

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

                                    let _response = client.load_component(LoadComponentRequest {
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

                                    let _response = client.unload_component(UnloadComponentRequest {
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
                                    let (component_id_opt, arguments) = if let Some(obj) = arguments_json.as_object() {
                                        let mut obj_clone = obj.clone();
                                        let comp_id = obj_clone.remove("componentId")
                                            .and_then(|v| v.as_str().map(|s| s.to_string()));
                                        (comp_id, Value::Object(obj_clone))
                                    } else {
                                        (None, req.arguments.unwrap_or(Value::Null))
                                    };

                                    let component_id = component_id_opt
                                        .ok_or_else(|| anyhow::anyhow!("Component ID not provided"))?;

                                    let response = client.call_component(CallComponentRequest {
                                        id: component_id,
                                        parameters: serde_json::to_string(&arguments)?,
                                        function_name: req.name,
                                    }).await?;

                                    if !response.get_ref().error.is_empty() {
                                        return Err(anyhow::anyhow!(response.get_ref().error.clone()));
                                    }

                                    let result_str = String::from_utf8(response.get_ref().result.clone())?;

                                    Ok(CallToolResponse {
                                        content: vec![ToolResponseContent::Text {
                                            text: result_str,
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

        tokio::select! {
            res = server.listen() => { res?; }
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
