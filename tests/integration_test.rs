use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::Full;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use lifecycle_proto::lifecycle::lifecycle_manager_service_client::LifecycleManagerServiceClient;
use lifecycle_proto::lifecycle::{
    CallComponentRequest, GetComponentRequest, ListComponentsRequest, LoadComponentRequest,
    UnloadComponentRequest,
};
use mcp_wasmtime::wasmtimed;
use oci_wasm::WasmClient;
use tempfile::TempDir;
use test_log::test;
use testcontainers::{core::WaitFor, runners::AsyncRunner, ContainerAsync, Image};
use tokio::net::TcpListener;
use tokio::time::sleep;
use tonic::transport::Channel;

const DOCKER_REGISTRY_PORT: u16 = 5000;

pub async fn find_open_port() -> Result<u16> {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        .await
        .context("failed to bind random port")?
        .local_addr()
        .map(|addr| addr.port())
        .context("failed to get local address from opened TCP socket")
}

#[derive(Default)]
struct DockerRegistry {
    _priv: (),
}

impl Image for DockerRegistry {
    fn name(&self) -> &str {
        "registry"
    }

    fn tag(&self) -> &str {
        "2"
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        vec![WaitFor::message_on_stderr("listening on")]
    }
}

async fn setup_registry() -> anyhow::Result<ContainerAsync<DockerRegistry>> {
    DockerRegistry::default()
        .start()
        .await
        .context("Failed to start docker registry")
}

async fn wait_for_client(port: u16) -> Result<LifecycleManagerServiceClient<Channel>> {
    let mut retries = 5;
    let mut last_error = None;

    while retries > 0 {
        let addr = format!("http://127.0.0.1:{}", port);
        match Channel::from_shared(addr.clone())?.connect().await {
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

async fn build_example_component() -> Result<PathBuf> {
    let top_level =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").context("CARGO_MANIFEST_DIR not set")?);
    // NOTE: This assumes we are using linux path separators and hasn't been tested on windows.
    let component_path =
        top_level.join("examples/fetch-rs/target/wasm32-wasip2/release/fetch_rs.wasm");

    let status = tokio::process::Command::new("cargo")
        .current_dir(top_level.join("examples/fetch-rs"))
        .args(["build", "--release", "--target", "wasm32-wasip2"])
        .status()
        .await
        .context("Failed to execute cargo component build")?;

    if !status.success() {
        anyhow::bail!("Failed to compile fetch-rs component");
    }

    if !component_path.exists() {
        anyhow::bail!(
            "Component file not found after build: {}",
            component_path.display()
        );
    }

    Ok(component_path)
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

async fn setup_daemon() -> Result<(LifecycleManagerServiceClient<Channel>, TempDir, u16)> {
    let port = find_open_port().await?;
    let addr = format!("127.0.0.1:{}", port);
    let tempdir = tempfile::tempdir()?;
    let daemon = wasmtimed::WasmtimeD::new_with_clients(
        addr.clone(),
        &tempdir,
        None,
        oci_client::Client::new(oci_client::client::ClientConfig {
            protocol: oci_client::client::ClientProtocol::Http,
            ..Default::default()
        }),
        reqwest::Client::default(),
    )
    .await
    .context("Failed to create WasmtimeD")?;

    tokio::spawn(async move {
        if let Err(e) = daemon.serve().await {
            tracing::error!("Daemon error: {}", e);
        }
    });

    let mut client = wait_for_client(port)
        .await
        .context("Failed to connect to daemon")?;

    let mut retries = 5;
    while retries > 0 {
        match cleanup_components(&mut client).await {
            Ok(_) => return Ok((client, tempdir, port)),
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
    let (mut client, _tempdir, _port) = setup_daemon().await?;

    let list_request = tonic::Request::new(ListComponentsRequest {});
    let list_response = client.list_components(list_request).await?;
    let initial_components = list_response.into_inner().ids;
    assert!(
        initial_components.is_empty(),
        "Expected no components initially"
    );

    let component_path = build_example_component().await?;

    let load_request = tonic::Request::new(LoadComponentRequest {
        path: format!("file://{}", component_path.to_str().unwrap()),
    });
    client.load_component(load_request).await?;

    let list_request = tonic::Request::new(ListComponentsRequest {});
    let list_response = client.list_components(list_request).await?;
    let components_after_load = list_response.into_inner().ids;
    assert_eq!(components_after_load.len(), 1);
    assert_eq!(components_after_load[0], "fetch_rs");

    let get_request = tonic::Request::new(GetComponentRequest {
        id: "fetch_rs".to_string(),
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

    // Copy the component to another name
    let mut component_path2 = component_path.clone();
    component_path2.set_file_name("fetch2.wasm");
    tokio::fs::copy(&component_path, &component_path2).await?;

    let load_request2 = tonic::Request::new(LoadComponentRequest {
        path: format!("file://{}", component_path2.to_str().unwrap()),
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
    assert!(error.message().contains("fetch_rs"));
    assert!(error.message().contains("fetch2"));

    Ok(())
}

// Helper function to start a simple HTTP server
async fn start_http_server(
    wasm_content: Vec<u8>,
) -> Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let wasm_bytes = Arc::new(wasm_content);

    let handle = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(stream);
            let wasm_bytes = wasm_bytes.clone();

            let service = service_fn(move |req: Request<hyper::body::Incoming>| {
                let wasm_bytes = wasm_bytes.clone();
                async move {
                    if req.uri().path() != "/fetch_rs.wasm" {
                        return Ok::<_, hyper::Error>(
                            Response::builder()
                                .status(404)
                                .body(Full::new(Bytes::from("Not Found")))
                                .unwrap(),
                        );
                    }
                    let response = Response::builder()
                        .status(200)
                        .header("Content-Type", "application/wasm")
                        .body(Full::new(Bytes::from(wasm_bytes.as_ref().clone())))
                        .unwrap();
                    Ok::<_, hyper::Error>(response)
                }
            });

            tokio::spawn(async move {
                if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                    eprintln!("Error serving connection: {:?}", err);
                }
            });
        }
    });

    Ok((addr, handle))
}

#[test(tokio::test)]
async fn test_load_component_from_http() -> Result<()> {
    let (mut client, _tempdir, _port) = setup_daemon().await?;

    // Build the test component
    let component_path = build_example_component().await?;

    // Read the component bytes
    let wasm_bytes = tokio::fs::read(&component_path).await?;

    // Start HTTP server
    let (addr, _server_handle) = start_http_server(wasm_bytes).await?;

    // Load from HTTP
    let http_url = format!("http://{}/fetch_rs.wasm", addr);
    let load_request = tonic::Request::new(LoadComponentRequest { path: http_url });
    client.load_component(load_request).await?;

    // Verify component was loaded
    let list_request = tonic::Request::new(ListComponentsRequest {});
    let list_response = client.list_components(list_request).await?;
    let components = list_response.into_inner().ids;
    assert!(components.contains(&"fetch_rs".to_string()));

    // Test calling the component
    let call_request = tonic::Request::new(CallComponentRequest {
        function_name: "fetch".to_string(),
        parameters: r#"{"url": "https://example.com/"}"#.to_string(),
    });
    let call_response = client.call_component(call_request).await?;
    let result = call_response.into_inner();
    assert!(result.error.is_empty());

    Ok(())
}

#[test(tokio::test)]
async fn test_load_component_from_oci() -> Result<()> {
    let (mut client, _tempdir, _port) = setup_daemon().await?;

    // Build the test component
    let component_path = build_example_component().await?;

    // Start OCI registry using testcontainers
    let container = setup_registry().await?;
    let registry_port = container.get_host_port_ipv4(DOCKER_REGISTRY_PORT).await?;
    let registry_url = format!("localhost:{}", registry_port);

    // Give the registry a moment to fully start
    sleep(Duration::from_millis(500)).await;

    // Read component bytes
    let (config, layer) = oci_wasm::WasmConfig::from_component(component_path, None).await?;

    // Create OCI client and push the component
    let oci_client = oci_client::Client::new(oci_client::client::ClientConfig {
        protocol: oci_client::client::ClientProtocol::Http,
        ..Default::default()
    });

    let wasm_client = WasmClient::new(oci_client);
    let reference = format!("{}/fetch_rs:latest", registry_url);
    let oci_reference: oci_client::Reference = reference.parse()?;

    // Push to registry
    wasm_client
        .push(
            &oci_reference,
            &oci_client::secrets::RegistryAuth::Anonymous,
            layer,
            config,
            None,
        )
        .await?;

    // Load from OCI
    let oci_url = format!("oci://{}", reference);
    let load_request = tonic::Request::new(LoadComponentRequest { path: oci_url });
    client.load_component(load_request).await?;

    // Verify component was loaded
    let list_request = tonic::Request::new(ListComponentsRequest {});
    let list_response = client.list_components(list_request).await?;
    let components = list_response.into_inner().ids;
    assert!(components.contains(&"fetch_rs".to_string()));

    Ok(())
}

#[test(tokio::test)]
async fn test_load_component_invalid_scheme() -> Result<()> {
    let (mut client, _tempdir, _port) = setup_daemon().await?;

    // Try to load with invalid scheme
    let load_request = tonic::Request::new(LoadComponentRequest {
        path: "ftp://example.com/component.wasm".to_string(),
    });

    let result = client.load_component(load_request).await;
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.message().contains("Unsupported component scheme"));

    Ok(())
}

#[test(tokio::test)]
async fn test_load_component_http_404() -> Result<()> {
    let (mut client, _tempdir, _port) = setup_daemon().await?;

    // Start HTTP server
    let (addr, _server_handle) = start_http_server(Vec::new()).await?;

    // Try to load from HTTP with 404
    let http_url = format!("http://{}/nonexistent.wasm", addr);
    let load_request = tonic::Request::new(LoadComponentRequest { path: http_url });

    let result = client.load_component(load_request).await;
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(
        error
            .message()
            .contains("Failed to download component from URL"),
        "Wrong error message found, got: {}",
        error.message()
    );

    Ok(())
}

#[test(tokio::test)]
async fn test_load_component_invalid_reference() -> Result<()> {
    let (mut client, _tempdir, _port) = setup_daemon().await?;

    // Try to load without scheme
    let load_request = tonic::Request::new(LoadComponentRequest {
        path: "not_a_valid_reference".to_string(),
    });

    let result = client.load_component(load_request).await;
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error.message().contains("Invalid component reference"));

    Ok(())
}
