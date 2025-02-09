use anyhow::Result;
use lifecycle_proto::lifecycle::lifecycle_manager_service_client::LifecycleManagerServiceClient;
use lifecycle_proto::lifecycle::{
    CallComponentRequest, GetComponentRequest, ListComponentsRequest, LoadComponentRequest,
    UnloadComponentRequest,
};
use mcp_sdk::types::{CallToolRequest, CallToolResponse, ToolDefinition, ToolResponseContent};
use serde_json::{json, Value};
use tonic::transport::Channel;
use tracing::{debug, error, info, instrument};

use crate::GrpcClient;

#[instrument(skip(grpc_client))]
pub async fn get_component_tools(grpc_client: &GrpcClient) -> Result<Vec<ToolDefinition>> {
    let mut client = grpc_client.lock().await;
    debug!("Listing components");
    let components = client
        .list_components(ListComponentsRequest {})
        .await?
        .into_inner();

    info!("Found {} components", components.ids.len());
    let mut tools = Vec::new();

    for id in components.ids {
        debug!("Getting component details for {}", id);
        let response = client
            .get_component(GetComponentRequest { id: id.clone() })
            .await?;

        if let Ok(schema) = serde_json::from_str::<Value>(&response.into_inner().details) {
            if let Some(arr) = schema.get("tools").and_then(|v| v.as_array()) {
                let tool_count = arr.len();
                debug!("Found {} tools in component {}", tool_count, id);
                for tool_json in arr {
                    if let Some(tool) = parse_tool_schema(tool_json) {
                        tools.push(tool);
                    }
                }
            }
        }
    }
    info!("Total tools collected: {}", tools.len());
    Ok(tools)
}

#[instrument(skip(client))]
pub(crate) async fn handle_load_component(
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

    info!("Loading component {} from path {}", id, path);
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

#[instrument(skip(client))]
pub(crate) async fn handle_unload_component(
    req: &CallToolRequest,
    client: &mut LifecycleManagerServiceClient<Channel>,
) -> Result<CallToolResponse> {
    let args = req.arguments.clone().unwrap_or(Value::Null);
    let id = args
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'id' in arguments"))?;

    info!("Unloading component {}", id);
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

#[instrument(skip(client))]
pub(crate) async fn handle_component_call(
    req: &CallToolRequest,
    client: &mut LifecycleManagerServiceClient<Channel>,
) -> Result<CallToolResponse> {
    let arguments = req.arguments.clone().unwrap_or(Value::Null);
    info!("Calling function {}", req.name);

    let response = client
        .call_component(CallComponentRequest {
            parameters: serde_json::to_string(&arguments)?,
            function_name: req.name.clone(),
        })
        .await?;

    if !response.get_ref().error.is_empty() {
        error!("Component call failed: {}", response.get_ref().error);
        return Err(anyhow::anyhow!(response.get_ref().error.clone()));
    }

    let result_str = String::from_utf8(response.get_ref().result.clone())?;
    debug!("Component call successful");

    Ok(CallToolResponse {
        content: vec![ToolResponseContent::Text { text: result_str }],
        is_error: None,
        meta: None,
    })
}

#[instrument]
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
    let input_schema = tool_json.get("inputSchema").cloned().unwrap_or(json!({}));

    debug!("Parsed tool schema for {}", name);

    Some(ToolDefinition {
        name,
        description,
        input_schema,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_request(name: &str, args: Value) -> CallToolRequest {
        CallToolRequest {
            name: name.to_string(),
            arguments: Some(args),
            meta: None,
        }
    }

    #[test]
    fn test_parse_tool_schema() {
        let tool_json = json!({
            "name": "test-tool",
            "description": "Test tool description",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "test": {"type": "string"}
                }
            }
        });

        let tool = parse_tool_schema(&tool_json).unwrap();

        assert_eq!(tool.name, "test-tool");
        assert_eq!(tool.description, Some("Test tool description".to_string()));
        assert_eq!(
            tool.input_schema,
            json!({
                "type": "object",
                "properties": {
                    "test": {"type": "string"}
                }
            })
        );
    }
}
