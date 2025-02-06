use anyhow::Result;
use mcp_sdk::types::{
    CallToolRequest, CallToolResponse, ListRequest, ToolDefinition, ToolsListResponse,
};
use serde_json::json;
use tokio::task::block_in_place;
use tracing::{debug, error, info};

use crate::components::{
    get_component_tools, handle_component_call, handle_load_component, handle_unload_component,
};
use crate::GrpcClient;

pub fn handle_tools_list(_req: ListRequest, grpc_client: GrpcClient) -> Result<ToolsListResponse> {
    debug!("Handling tools list request");
    block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            let mut tools = get_component_tools(&grpc_client).await?;
            tools.extend(get_builtin_tools());
            info!("Retrieved {} tools", tools.len());
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
    info!("Handling tool call for: {}", req.name);
    block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            let mut client = grpc_client.lock().await;
            let result = match req.name.as_str() {
                "load-component" => handle_load_component(&req, &mut client).await,
                "unload-component" => handle_unload_component(&req, &mut client).await,
                _ => handle_component_call(&req, &mut client).await,
            };
            if let Err(ref e) = result {
                error!("Tool call failed: {}", e);
            }
            result
        })
    })
}

fn get_builtin_tools() -> Vec<ToolDefinition> {
    debug!("Getting builtin tools");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_builtin_tools() {
        let tools = get_builtin_tools();
        assert_eq!(tools.len(), 2);
        assert!(tools.iter().any(|t| t.name == "load-component"));
        assert!(tools.iter().any(|t| t.name == "unload-component"));
    }
}
