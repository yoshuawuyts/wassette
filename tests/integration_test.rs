use std::env;
use std::time::Duration;

use anyhow::{Context, Result};
use lifecycle_proto::lifecycle::lifecycle_manager_service_client::LifecycleManagerServiceClient;
use lifecycle_proto::lifecycle::{
    CallComponentRequest, GetComponentRequest, ListComponentsRequest, LoadComponentRequest,
    UnloadComponentRequest,
};
use mcp_wasmtime::wasmtimed;
use test_log::test;
use tokio::time::sleep;
use tonic::transport::Channel;

async fn wait_for_client() -> Result<LifecycleManagerServiceClient<Channel>> {
    let mut retries = 5;
    let mut last_error = None;

    while retries > 0 {
        match Channel::from_static("http://[::1]:50051").connect().await {
            Ok(channel) => return Ok(LifecycleManagerServiceClient::new(channel)),
            Err(e) => {
                last_error = Some(e);
                retries -= 1;
                if retries > 0 {
                    sleep(Duration::from_millis(200)).await;
                }
            }
        }
    }

    Err(last_error.unwrap().into())
}

async fn cleanup_components(client: &mut LifecycleManagerServiceClient<Channel>) -> Result<()> {
    let list_response = client.list_components(ListComponentsRequest {}).await?;
    for id in list_response.into_inner().ids {
        client
            .unload_component(UnloadComponentRequest { id: id.clone() })
            .await
            .with_context(|| format!("Failed to unload component {}", id))?;
    }
    Ok(())
}

async fn setup_daemon() -> Result<LifecycleManagerServiceClient<Channel>> {
    let addr = "[::1]:50051";
    let daemon = wasmtimed::WasmtimeD::new(addr.to_string(), "sqlite::memory:")
        .await
        .context("Failed to create WasmtimeD")?;

    tokio::spawn(async move {
        if let Err(e) = daemon.serve().await {
            tracing::error!("Daemon error: {}", e);
        }
    });

    let mut client = wait_for_client()
        .await
        .context("Failed to connect to daemon")?;

    let mut retries = 5;
    while retries > 0 {
        match cleanup_components(&mut client).await {
            Ok(_) => return Ok(client),
            Err(_) if retries > 1 => {
                retries -= 1;
                sleep(Duration::from_millis(200)).await;
            }
            Err(e) => return Err(e),
        }
    }

    Err(anyhow::anyhow!("Failed to verify daemon is working"))
}

#[test(tokio::test)]
async fn test_fetch_component_workflow() -> Result<()> {
    let mut client = setup_daemon().await?;

    let list_request = tonic::Request::new(ListComponentsRequest {});
    let list_response = client.list_components(list_request).await?;
    let initial_components = list_response.into_inner().ids;
    assert!(
        initial_components.is_empty(),
        "Expected no components initially"
    );

    let cwd = env::current_dir()?;
    let component_path = cwd.join("examples/fetch-rs/target/wasm32-wasip2/release/fetch_rs.wasm");

    let status = std::process::Command::new("cargo")
        .current_dir(cwd.join("examples/fetch-rs"))
        .args(["build", "--release", "--target", "wasm32-wasip2"])
        .status()
        .expect("Failed to compile fetch-rs component");

    if !status.success() {
        anyhow::bail!("Failed to compile fetch-rs component");
    }

    let load_request = tonic::Request::new(LoadComponentRequest {
        id: "fetch".to_string(),
        path: component_path.to_str().unwrap().to_string(),
    });
    client.load_component(load_request).await?;

    let list_request = tonic::Request::new(ListComponentsRequest {});
    let list_response = client.list_components(list_request).await?;
    let components_after_load = list_response.into_inner().ids;
    assert_eq!(components_after_load.len(), 1);
    assert_eq!(components_after_load[0], "fetch");

    let get_request = tonic::Request::new(GetComponentRequest {
        id: "fetch".to_string(),
    });
    let get_response = client.get_component(get_request).await?;
    let component_details = get_response.into_inner().details;
    let schema: serde_json::Value = serde_json::from_str(&component_details)?;
    assert!(schema["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["name"] == "fetch"));

    let call_request = tonic::Request::new(CallComponentRequest {
        function_name: "fetch".to_string(),
        parameters: r#"{"url": "https://example.com/"}"#.to_string(),
    });
    let call_response = client.call_component(call_request).await?;
    let result = call_response.into_inner();
    assert!(result.error.is_empty());

    let response_body = String::from_utf8(result.result).expect("Invalid UTF-8 in response");
    assert!(response_body.contains("Example Domain"));
    assert!(response_body.contains("This domain is for use in illustrative examples in documents"));

    let load_request1 = tonic::Request::new(LoadComponentRequest {
        id: "fetch1".to_string(),
        path: component_path.to_str().unwrap().to_string(),
    });
    client.load_component(load_request1).await?;

    let load_request2 = tonic::Request::new(LoadComponentRequest {
        id: "fetch2".to_string(),
        path: component_path.to_str().unwrap().to_string(),
    });
    client.load_component(load_request2).await?;

    let call_request = tonic::Request::new(CallComponentRequest {
        function_name: "fetch".to_string(),
        parameters: r#"{"url": "https://example.com/"}"#.to_string(),
    });
    let call_result = client.call_component(call_request).await;

    assert!(call_result.is_err());
    let error = call_result.unwrap_err();
    assert!(error
        .message()
        .contains("Multiple components found for tool 'fetch'"));
    assert!(error.message().contains("fetch1"));
    assert!(error.message().contains("fetch2"));

    Ok(())
}
