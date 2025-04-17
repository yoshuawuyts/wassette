use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use lifecycle_proto::lifecycle::lifecycle_manager_service_client::LifecycleManagerServiceClient;
use mcp_wasmtime_server::{
    handle_prompts_list, handle_resources_list, handle_tools_call, handle_tools_list, GrpcClient,
};
use rmcp::model::{
    CallToolRequestParam, CallToolResult, ErrorData, ListPromptsResult, ListResourcesResult,
    ListToolsResult, PaginatedRequestParamInner, ServerCapabilities, ServerInfo, ToolsCapability,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::SseServer;
use rmcp::ServerHandler;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;

mod database;
mod wasmtimed;

const BIND_ADDRESS: &str = "127.0.0.1:9001";

#[derive(Clone)]
pub struct McpServer {
    grpc_client: GrpcClient,
}

impl McpServer {
    pub fn new(grpc_client: GrpcClient) -> Self {
        Self { grpc_client }
    }
}

#[allow(refining_impl_trait_reachable)]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: Some(true),
                }),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn call_tool<'a>(
        &'a self,
        params: CallToolRequestParam,
        _ctx: RequestContext<RoleServer>,
    ) -> Pin<Box<dyn Future<Output = Result<CallToolResult, ErrorData>> + Send + 'a>> {
        Box::pin(async move {
            let result = handle_tools_call(params, self.grpc_client.clone()).await;
            match result {
                Ok(value) => serde_json::from_value(value).map_err(|e| {
                    ErrorData::parse_error(format!("Failed to parse result: {}", e), None)
                }),
                Err(err) => Err(ErrorData::parse_error(err.to_string(), None)),
            }
        })
    }

    fn list_tools<'a>(
        &'a self,
        _params: Option<PaginatedRequestParamInner>,
        _ctx: RequestContext<RoleServer>,
    ) -> Pin<Box<dyn Future<Output = Result<ListToolsResult, ErrorData>> + Send + 'a>> {
        Box::pin(async move {
            let result = handle_tools_list(serde_json::Value::Null, self.grpc_client.clone()).await;
            match result {
                Ok(value) => serde_json::from_value(value).map_err(|e| {
                    ErrorData::parse_error(format!("Failed to parse result: {}", e), None)
                }),
                Err(err) => Err(ErrorData::parse_error(err.to_string(), None)),
            }
        })
    }

    fn list_prompts<'a>(
        &'a self,
        _params: Option<PaginatedRequestParamInner>,
        _ctx: RequestContext<RoleServer>,
    ) -> Pin<Box<dyn Future<Output = Result<ListPromptsResult, ErrorData>> + Send + 'a>> {
        Box::pin(async move {
            let result = handle_prompts_list(serde_json::Value::Null).await;
            match result {
                Ok(value) => serde_json::from_value(value).map_err(|e| {
                    ErrorData::parse_error(format!("Failed to parse result: {}", e), None)
                }),
                Err(err) => Err(ErrorData::parse_error(err.to_string(), None)),
            }
        })
    }

    fn list_resources<'a>(
        &'a self,
        _params: Option<PaginatedRequestParamInner>,
        _ctx: RequestContext<RoleServer>,
    ) -> Pin<Box<dyn Future<Output = Result<ListResourcesResult, ErrorData>> + Send + 'a>> {
        Box::pin(async move {
            let result = handle_resources_list(serde_json::Value::Null).await;
            match result {
                Ok(value) => serde_json::from_value(value).map_err(|e| {
                    ErrorData::parse_error(format!("Failed to parse result: {}", e), None)
                }),
                Err(err) => Err(ErrorData::parse_error(err.to_string(), None)),
            }
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug".to_string().into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let database_url = database::resolve_database_url().await?;

    // GRPC server address for wasmtimed
    let grpc_addr = "[::1]:50051";
    let daemon = wasmtimed::WasmtimeD::new(grpc_addr.to_string(), &database_url).await?;

    // Create a cancellation token for coordinating shutdown
    let daemon_shutdown_token = CancellationToken::new();
    let daemon_token_clone = daemon_shutdown_token.clone();

    tokio::spawn(async move {
        let daemon_serve = daemon.serve();
        tokio::select! {
            result = daemon_serve => {
                if let Err(e) = result {
                    tracing::error!("Daemon error: {}", e);
                }
            }
            _ = daemon_token_clone.cancelled() => {
                tracing::info!("Daemon shutting down due to cancellation");
            }
        }
    });

    // Initialize gRPC client
    let grpc_client =
        LifecycleManagerServiceClient::connect(format!("http://{}", grpc_addr)).await?;
    let grpc_client = Arc::new(tokio::sync::Mutex::new(grpc_client));

    let server = McpServer::new(grpc_client);

    // Serve the SSE server and get the cancellation token
    tracing::info!("Starting MCP server on {}", BIND_ADDRESS);
    let ct = SseServer::serve(BIND_ADDRESS.parse().unwrap())
        .await?
        .with_service(move || server.clone());

    // Setup Ctrl+C handler
    tokio::signal::ctrl_c().await?;
    ct.cancel();
    daemon_shutdown_token.cancel();

    tracing::info!("MCP server shutting down");
    Ok(())
}
