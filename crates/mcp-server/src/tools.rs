use std::borrow::Cow;
use std::sync::Arc;

use anyhow::Result;
use rmcp::model::{CallToolRequestParam, CallToolResult, Content, Tool};
use rmcp::{Peer, RoleServer};
use serde_json::{json, Value};
use tracing::{debug, error, info};

use crate::components::{
    get_component_tools, handle_component_call, handle_load_component, handle_unload_component,
};
use crate::GrpcClient;

pub async fn handle_tools_list(_req: Value, grpc_client: GrpcClient) -> Result<Value> {
    debug!("Handling tools list request");

    let mut tools = get_component_tools(&grpc_client).await?;
    tools.extend(get_builtin_tools());
    info!("Retrieved {} tools", tools.len());

    let response = rmcp::model::ListToolsResult {
        tools,
        next_cursor: None,
    };

    Ok(serde_json::to_value(response)?)
}

pub async fn handle_tools_call(
    req: CallToolRequestParam,
    grpc_client: GrpcClient,
    server_peer: Option<Peer<RoleServer>>,
) -> Result<Value> {
    // Extract the method name as a string
    let method_name = req.name.to_string();
    info!("Handling tool call for: {}", method_name);

    let mut client = grpc_client.lock().await;

    let result = match method_name.as_str() {
        "load-component" => handle_load_component(&req, &mut client, server_peer).await,
        "unload-component" => handle_unload_component(&req, &mut client, server_peer).await,
        _ => handle_component_call(&req, &mut client).await,
    };

    if let Err(ref e) = result {
        error!("Tool call failed: {}", e);
    }

    match result {
        Ok(result) => Ok(serde_json::to_value(result)?),
        Err(e) => {
            // Return an error result with explicit type
            let error_text = format!("Error: {}", e);
            let mut contents = Vec::new();
            contents.push(Content::text(error_text));

            let error_result = CallToolResult {
                content: contents,
                is_error: Some(true),
            };
            Ok(serde_json::to_value(error_result)?)
        }
    }
}

fn get_builtin_tools() -> Vec<Tool> {
    debug!("Getting builtin tools");
    vec![
        Tool {
            name: Cow::Borrowed("load-component"),
            description: Cow::Borrowed(
                "Dynamically loads a new WebAssembly component. Arguments: id (string), path (string)"
            ),
            input_schema: Arc::new(serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "path": {"type": "string"}
                },
                "required": ["id", "path"]
            })).unwrap_or_default()),
        },
        Tool {
            name: Cow::Borrowed("unload-component"),
            description: Cow::Borrowed(
                "Dynamically unloads a WebAssembly component. Argument: id (string)"
            ),
            input_schema: Arc::new(serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string"}
                },
                "required": ["id"]
            })).unwrap_or_default()),
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
