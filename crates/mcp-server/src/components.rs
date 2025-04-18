use std::borrow::Cow;
use std::sync::Arc;

use anyhow::Result;
use lifecycle_proto::lifecycle::lifecycle_manager_service_client::LifecycleManagerServiceClient;
use lifecycle_proto::lifecycle::{
    CallComponentRequest, GetComponentRequest, ListComponentsRequest, LoadComponentRequest,
    UnloadComponentRequest,
};
use rmcp::model::{CallToolRequestParam, CallToolResult, Content, Tool};
use rmcp::{Peer, RoleServer};
use serde_json::{json, Value};
use tonic::transport::Channel;
use tracing::{debug, error, info, instrument};

use crate::GrpcClient;

#[instrument(skip(grpc_client))]
pub async fn get_component_tools(grpc_client: &GrpcClient) -> Result<Vec<Tool>> {
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
    req: &CallToolRequestParam,
    client: &mut LifecycleManagerServiceClient<Channel>,
    server_peer: Option<Peer<RoleServer>>,
) -> Result<CallToolResult> {
    let args = extract_args_from_request(req)?;

    let id = args
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required argument: 'id'"))?;
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required argument: 'path'"))?;

    info!("Loading component {} from path {}", id, path);

    let response = client
        .load_component(LoadComponentRequest {
            id: id.to_string(),
            path: path.to_string(),
        })
        .await;

    match response {
        Ok(_) => {
            let status_text = serde_json::to_string(&json!({
                "status": "component loaded",
                "id": id
            }))?;

            let mut contents = Vec::new();
            contents.push(Content::text(status_text));

            if let Some(peer) = server_peer {
                if let Err(e) = peer.notify_tool_list_changed().await {
                    error!("Failed to send tool list change notification: {}", e);
                } else {
                    info!(
                        "Sent tool list changed notification after loading component {}",
                        id
                    );
                }
            }

            Ok(CallToolResult {
                content: contents,
                is_error: None,
            })
        }
        Err(e) => {
            error!("Failed to load component: {}", e);
            Err(anyhow::anyhow!(
                "Failed to load component: {}. Error: {}",
                path,
                e
            ))
        }
    }
}

#[instrument(skip(client))]
pub(crate) async fn handle_unload_component(
    req: &CallToolRequestParam,
    client: &mut LifecycleManagerServiceClient<Channel>,
    server_peer: Option<Peer<RoleServer>>,
) -> Result<CallToolResult> {
    let args = extract_args_from_request(req)?;

    let id = args
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'id' in arguments"))?;

    info!("Unloading component {}", id);
    let _response = client
        .unload_component(UnloadComponentRequest { id: id.to_string() })
        .await?;

    let status_text = serde_json::to_string(&json!({
        "status": "component unloaded",
        "id": id
    }))?;

    let mut contents = Vec::new();
    contents.push(Content::text(status_text));

    if let Some(peer) = server_peer {
        if let Err(e) = peer.notify_tool_list_changed().await {
            error!("Failed to send tool list change notification: {}", e);
        } else {
            info!(
                "Sent tool list changed notification after unloading component {}",
                id
            );
        }
    }

    Ok(CallToolResult {
        content: contents,
        is_error: None,
    })
}

#[instrument(skip(client))]
pub(crate) async fn handle_component_call(
    req: &CallToolRequestParam,
    client: &mut LifecycleManagerServiceClient<Channel>,
) -> Result<CallToolResult> {
    let args = extract_args_from_request(req)?;

    let method_name = req.name.to_string();
    info!("Calling function {}", method_name);

    let response = client
        .call_component(CallComponentRequest {
            parameters: serde_json::to_string(&args)?,
            function_name: method_name,
        })
        .await?;

    if !response.get_ref().error.is_empty() {
        error!("Component call failed: {}", response.get_ref().error);
        return Err(anyhow::anyhow!(response.get_ref().error.clone()));
    }

    let result_str = String::from_utf8(response.get_ref().result.clone())?;
    debug!("Component call successful");

    let mut contents = Vec::new();
    contents.push(Content::text(result_str));

    Ok(CallToolResult {
        content: contents,
        is_error: None,
    })
}

fn extract_args_from_request(req: &CallToolRequestParam) -> Result<serde_json::Map<String, Value>> {
    let params_value = serde_json::to_value(&req.arguments)?;

    match params_value {
        Value::Object(map) => Ok(map),
        _ => Err(anyhow::anyhow!(
            "Parameters are not in expected object format"
        )),
    }
}

#[instrument]
fn parse_tool_schema(tool_json: &Value) -> Option<Tool> {
    let name = tool_json
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("<unnamed>");

    let description = tool_json
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("No description available");

    let input_schema = tool_json.get("inputSchema").cloned().unwrap_or(json!({}));

    debug!("Parsed tool schema for {}", name);

    Some(Tool {
        name: Cow::Owned(name.to_string()),
        description: Cow::Owned(description.to_string()),
        input_schema: Arc::new(serde_json::from_value(input_schema).unwrap_or_default()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(tool.description, "Test tool description");

        let schema_json = serde_json::to_value(&*tool.input_schema).unwrap();
        let expected = json!({
            "type": "object",
            "properties": {
                "test": {"type": "string"}
            }
        });
        assert_eq!(schema_json, expected);
    }
}
