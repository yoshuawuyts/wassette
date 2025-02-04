use anyhow::Result;
use mcp_sdk::types::{
    CallToolRequest, CallToolResponse, ListRequest, ToolDefinition, ToolsListResponse,
};
use serde_json::json;
use tokio::task::block_in_place;

use crate::components::{
    get_component_tools, handle_component_call, handle_load_component, handle_unload_component,
};
use crate::GrpcClient;

pub fn handle_tools_list(_req: ListRequest, grpc_client: GrpcClient) -> Result<ToolsListResponse> {
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

pub fn handle_tools_call(
    req: CallToolRequest,
    grpc_client: GrpcClient,
) -> Result<CallToolResponse> {
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
