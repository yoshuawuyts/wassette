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
    let (component_id, arguments) = extract_component_id_and_args(req)?;
    info!(
        "Calling component {} with function {}",
        component_id, req.name
    );

    let response = client
        .call_component(CallComponentRequest {
            id: component_id.clone(),
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
    let mut input_schema = tool_json.get("inputSchema").cloned().unwrap_or(json!({}));

    add_component_id(&mut input_schema);
    debug!("Parsed tool schema for {}", name);

    Some(ToolDefinition {
        name,
        description,
        input_schema,
    })
}

#[instrument]
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
    debug!("Extracted component ID: {}", component_id);
    Ok((component_id, arguments))
}

#[instrument]
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

        assert_eq!(
            tool.input_schema,
            json!({
                "type": "object",
                "properties": {
                    "test": {"type": "string"},
                    "componentId": {"type": "string"}
                },
                "required": ["componentId"]
            })
        )
    }

    #[test]
    fn test_extract_component_id_and_args() {
        let req = setup_test_request(
            "test-function",
            json!({
                "componentId": "test-id",
                "param1": "value1"
            }),
        );

        let (id, args) = extract_component_id_and_args(&req).unwrap();
        assert_eq!(id, "test-id");
        assert_eq!(args.get("param1").unwrap(), "value1");
        assert!(args.get("componentId").is_none());
    }

    #[test]
    #[should_panic(expected = "Component ID not provided")]
    fn test_extract_component_id_missing() {
        let req = setup_test_request(
            "test-function",
            json!({
                "param1": "value1"
            }),
        );
        extract_component_id_and_args(&req).unwrap();
    }

    #[test]
    fn test_add_component_id() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "test": {"type": "string"}
            },
            "required": ["test"]
        });

        add_component_id(&mut schema);

        assert_eq!(
            schema,
            json!({
                "type": "object",
                "properties": {
                    "test": {"type": "string"},
                    "componentId": {"type": "string"}
                },
                "required": ["test", "componentId"]
            })
        )
    }
}
