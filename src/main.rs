use anyhow::Result;
use mcp_sdk::server::Server;
use mcp_sdk::transport::ServerStdioTransport;
use mcp_sdk::types::{
    CallToolRequest, CallToolResponse, ListRequest, PromptsListResponse, ResourcesListResponse,
    ServerCapabilities, ToolDefinition, ToolResponseContent, ToolsListResponse,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::task::block_in_place;
use tonic::transport::Channel;

pub mod lifecycle {
    tonic::include_proto!("lifecycle");
}

use lifecycle::{
    lifecycle_manager_service_client::LifecycleManagerServiceClient, CallComponentRequest,
    GetComponentRequest, ListComponentsRequest, LoadComponentRequest, UnloadComponentRequest,
};

type GrpcClient = Arc<tokio::sync::Mutex<LifecycleManagerServiceClient<Channel>>>;

pub struct Client {
    transport: ServerStdioTransport,
    grpc_client: GrpcClient,
}

impl Client {
    pub async fn new(grpc_addr: String) -> Result<Self, Box<dyn std::error::Error>> {
        let grpc_client =
            LifecycleManagerServiceClient::connect(format!("http://{}", grpc_addr)).await?;
        Ok(Self {
            transport: ServerStdioTransport,
            grpc_client: Arc::new(tokio::sync::Mutex::new(grpc_client)),
        })
    }

    pub async fn serve(self) -> Result<(), Box<dyn std::error::Error>> {
        let server = self.build_server();
        tokio::select! {
            res = server.listen() => { res?; }
        }
        Ok(())
    }

    fn build_server(&self) -> Server<ServerStdioTransport> {
        let transport = self.transport.clone();
        let grpc_client = self.grpc_client.clone();

        Server::builder(transport)
            .capabilities(ServerCapabilities {
                tools: Some(json!({"listChanged": true})),
                ..Default::default()
            })
            .request_handler("tools/list", {
                let grpc_client = grpc_client.clone();
                move |req| handle_tools_list(req, grpc_client.clone())
            })
            .request_handler("tools/call", {
                let grpc_client = grpc_client.clone();
                move |req| handle_tools_call(req, grpc_client.clone())
            })
            .request_handler("prompts/list", |req| handle_prompts_list(req))
            .request_handler("resources/list", |req| handle_resources_list(req))
            .build()
    }
}

fn handle_tools_list(_req: ListRequest, grpc_client: GrpcClient) -> Result<ToolsListResponse> {
    block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            let mut tools = get_component_tools(&grpc_client).await?;
            tools.extend(get_builtin_tools());
            Ok(ToolsListResponse {
                tools,
                next_cursor: None,
                meta: None,
            })
        })
    })
}

async fn get_component_tools(grpc_client: &GrpcClient) -> Result<Vec<ToolDefinition>> {
    let mut client = grpc_client.lock().await;
    let components = client
        .list_components(ListComponentsRequest {})
        .await?
        .into_inner();
    let mut tools = Vec::new();

    for id in components.ids {
        let response = client
            .get_component(GetComponentRequest { id: id.clone() })
            .await?;

        if let Ok(schema) = serde_json::from_str::<Value>(&response.into_inner().details) {
            if let Some(arr) = schema.get("tools").and_then(|v| v.as_array()) {
                for tool_json in arr {
                    if let Some(tool) = parse_tool_schema(tool_json) {
                        tools.push(tool);
                    }
                }
            }
        }
    }
    Ok(tools)
}

fn parse_tool_schema(tool_json: &Value) -> Option<ToolDefinition> {
    let name = tool_json
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("<unnamed>")
        .to_string();
    let description = tool_json
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let mut input_schema = tool_json.get("inputSchema").cloned().unwrap_or(json!({}));

    add_component_id(&mut input_schema);

    Some(ToolDefinition {
        name,
        description,
        input_schema,
    })
}

fn get_builtin_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
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
        },
        ToolDefinition {
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
        },
    ]
}

fn handle_tools_call(req: CallToolRequest, grpc_client: GrpcClient) -> Result<CallToolResponse> {
    block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            let mut client = grpc_client.lock().await;
            match req.name.as_str() {
                "load-component" => handle_load_component(&req, &mut client).await,
                "unload-component" => handle_unload_component(&req, &mut client).await,
                _ => handle_component_call(&req, &mut client).await,
            }
        })
    })
}

async fn handle_load_component(
    req: &CallToolRequest,
    client: &mut LifecycleManagerServiceClient<Channel>,
) -> Result<CallToolResponse> {
    let args = req.arguments.clone().unwrap_or(Value::Null);
    let id = args
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'id' in arguments"))?;
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'path' in arguments"))?;

    let _response = client
        .load_component(LoadComponentRequest {
            id: id.to_string(),
            path: path.to_string(),
        })
        .await?;

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

async fn handle_unload_component(
    req: &CallToolRequest,
    client: &mut LifecycleManagerServiceClient<Channel>,
) -> Result<CallToolResponse> {
    let args = req.arguments.clone().unwrap_or(Value::Null);
    let id = args
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'id' in arguments"))?;

    let _response = client
        .unload_component(UnloadComponentRequest { id: id.to_string() })
        .await?;

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

async fn handle_component_call(
    req: &CallToolRequest,
    client: &mut LifecycleManagerServiceClient<Channel>,
) -> Result<CallToolResponse> {
    let (component_id, arguments) = extract_component_id_and_args(req)?;

    let response = client
        .call_component(CallComponentRequest {
            id: component_id,
            parameters: serde_json::to_string(&arguments)?,
            function_name: req.name.clone(),
        })
        .await?;

    if !response.get_ref().error.is_empty() {
        return Err(anyhow::anyhow!(response.get_ref().error.clone()));
    }

    let result_str = String::from_utf8(response.get_ref().result.clone())?;

    Ok(CallToolResponse {
        content: vec![ToolResponseContent::Text { text: result_str }],
        is_error: None,
        meta: None,
    })
}

fn handle_prompts_list(_req: ListRequest) -> Result<PromptsListResponse> {
    Ok(PromptsListResponse {
        prompts: vec![],
        next_cursor: None,
        meta: None,
    })
}

fn handle_resources_list(_req: ListRequest) -> Result<ResourcesListResponse> {
    Ok(ResourcesListResponse {
        resources: vec![],
        next_cursor: None,
        meta: None,
    })
}

fn extract_component_id_and_args(req: &CallToolRequest) -> Result<(String, Value)> {
    let arguments_json = req.arguments.clone().unwrap_or(Value::Null);
    let (component_id, arguments) = if let Some(obj) = arguments_json.as_object() {
        let mut obj_clone = obj.clone();
        let comp_id = obj_clone.remove("componentId")
            .or_else(|| obj_clone.remove("id"))
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .ok_or_else(|| anyhow::anyhow!("Component ID not provided. Please provide 'componentId' or 'id' in the arguments"))?;
        (comp_id, Value::Object(obj_clone))
    } else {
        return Err(anyhow::anyhow!(
            "Arguments must be an object containing 'componentId' or 'id'"
        ));
    };
    Ok((component_id, arguments))
}

fn add_component_id(input_schema: &mut Value) {
    if let Some(map) = input_schema.as_object_mut() {
        if !map.contains_key("properties") {
            map.insert("properties".to_string(), json!({}));
        }

        if let Some(props) = map.get_mut("properties").and_then(|v| v.as_object_mut()) {
            props.insert("componentId".to_string(), json!({"type": "string"}));
        }

        if !map.contains_key("required") {
            map.insert("required".to_string(), json!([]));
        }

        if let Some(required) = map.get_mut("required").and_then(|v| v.as_array_mut()) {
            if !required.contains(&json!("componentId")) {
                required.push(json!("componentId"));
            }
        }

        map.insert("type".to_string(), json!("object"));
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
