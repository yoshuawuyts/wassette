use std::process::{Child, Command};
use std::time::Duration;
use std::{env, fs};

use anyhow::Result;
use lifecycle_proto::lifecycle::lifecycle_manager_service_client::LifecycleManagerServiceClient;
use lifecycle_proto::lifecycle::{
    CallComponentRequest, GetComponentRequest, ListComponentsRequest, LoadComponentRequest,
};
use tempfile::TempDir;
use test_log::test;
use tokio::time::sleep;
use tonic::transport::Channel;

struct WasmtimedProcess {
    child: Child,
    _temp_dir: TempDir,
}

impl Drop for WasmtimedProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

async fn start_wasmtimed() -> Result<WasmtimedProcess> {
    // Kill any existing wasmtimed processes
    let _ = Command::new("pkill").args(["-f", "wasmtimed"]).status();

    let cwd = env::current_dir()?;
    let fetch_rs_dir = cwd.join("examples/fetch-rs");

    let status = Command::new("cargo")
        .current_dir(&fetch_rs_dir)
        .args(["component", "build", "--release"])
        .status()
        .expect("Failed to compile fetch-rs component");

    if !status.success() {
        anyhow::bail!("Failed to compile fetch-rs component");
    }

    let temp_dir = TempDir::new_in(".")?;
    let db_path = temp_dir.path().join("components.db");
    fs::write(&db_path, "")?;

    let child = Command::new(env!("CARGO_BIN_EXE_wasmtimed"))
        .current_dir(temp_dir.path())
        .env("DATABASE_URL", "sqlite:components.db")
        .env("RUST_LOG", "debug")
        .spawn()
        .expect("Failed to start wasmtimed");
    sleep(Duration::from_secs(1)).await;
    Ok(WasmtimedProcess {
        child,
        _temp_dir: temp_dir,
    })
}

async fn create_client() -> Result<LifecycleManagerServiceClient<Channel>> {
    let channel = Channel::from_static("http://[::1]:50051").connect().await?;
    Ok(LifecycleManagerServiceClient::new(channel))
}

#[test(tokio::test)]
async fn test_fetch_component_workflow() -> Result<()> {
    let _wasmtimed = start_wasmtimed().await?;
    let mut client = create_client().await?;

    let list_request = tonic::Request::new(ListComponentsRequest {});
    let list_response = client.list_components(list_request).await?;
    let initial_components = list_response.into_inner().ids;
    assert!(initial_components.is_empty());

    let cwd = env::current_dir()?;
    let component_path = cwd.join("examples/fetch-rs/target/wasm32-wasip1/release/fetch_rs.wasm");
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
        id: "fetch".to_string(),
        function_name: "fetch".to_string(),
        parameters: r#"{"url": "https://example.com/"}"#.to_string(),
    });
    let call_response = client.call_component(call_request).await?;
    let result = call_response.into_inner();
    assert!(result.error.is_empty());

    let response_body = String::from_utf8(result.result).expect("Invalid UTF-8 in response");
    println!("Response body: {}", response_body);
    assert!(response_body.contains("Example Domain"));
    assert!(response_body.contains("This domain is for use in illustrative examples in documents"));

    Ok(())
}
