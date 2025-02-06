use std::sync::Arc;

use anyhow::Result;
use lifecycle_proto::lifecycle::lifecycle_manager_service_client::LifecycleManagerServiceClient;
use mcp_sdk::server::Server;
use mcp_sdk::transport::ServerStdioTransport;
use mcp_sdk::types::ServerCapabilities;
use mcp_wasmtime_server::{
    handle_prompts_list, handle_resources_list, handle_tools_call, handle_tools_list,
};
use serde_json::json;
use tonic::transport::Channel;

mod wasmtimed;

type GrpcClient = Arc<tokio::sync::Mutex<LifecycleManagerServiceClient<Channel>>>;

pub struct Client {
    transport: ServerStdioTransport,
    grpc_client: GrpcClient,
}

impl Client {
    pub async fn new(grpc_addr: String) -> Result<Self> {
        let grpc_client =
            LifecycleManagerServiceClient::connect(format!("http://{}", grpc_addr)).await?;
        Ok(Self {
            transport: ServerStdioTransport,
            grpc_client: Arc::new(tokio::sync::Mutex::new(grpc_client)),
        })
    }

    pub async fn serve(self) -> Result<()> {
        let server = self.build_server();
        tokio::select! {
            res = server.listen() => { res?; }
        }
        Ok(())
    }

    fn build_server(&self) -> Server<ServerStdioTransport> {
        let transport = self.transport.clone();
        let grpc_client = self.grpc_client.clone();

        Server::builder(transport)
            .capabilities(ServerCapabilities {
                tools: Some(json!({"listChanged": true})),
                ..Default::default()
            })
            .request_handler("tools/list", {
                let grpc_client = grpc_client.clone();
                move |req| handle_tools_list(req, grpc_client.clone())
            })
            .request_handler("tools/call", {
                let grpc_client = grpc_client.clone();
                move |req| handle_tools_call(req, grpc_client.clone())
            })
            .request_handler("prompts/list", handle_prompts_list)
            .request_handler("resources/list", handle_resources_list)
            .build()
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(std::io::stderr)
        .init();

    let database_path =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:components.db".to_string());

    let addr = "[::1]:50051";
    let daemon = wasmtimed::WasmtimeD::new(addr.to_string(), &database_path).await?;

    tokio::spawn(async move {
        if let Err(e) = daemon.serve().await {
            tracing::error!("Daemon error: {}", e);
        }
    });

    let client = Client::new(addr.to_string()).await?;
    client.serve().await
}
