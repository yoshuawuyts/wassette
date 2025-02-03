use std::sync::Arc;
use tonic::{transport::Server, Request, Response, Status};
use lifecycle_manager::LifecycleManager;
use wasmtime;
use component2json::component_exports_to_json_schema;

pub mod lifecycle {
    tonic::include_proto!("lifecycle");
}

use lifecycle::{
    lifecycle_manager_service_server::{LifecycleManagerService, LifecycleManagerServiceServer},
    LoadComponentRequest, LoadComponentResponse,
    UnloadComponentRequest, UnloadComponentResponse,
    GetComponentRequest, GetComponentResponse,
    ListComponentsRequest, ListComponentsResponse,
};

#[derive(Clone)]
struct LifecycleManagerServiceImpl {
    manager: Arc<LifecycleManager>,
}

#[tonic::async_trait]
impl LifecycleManagerService for LifecycleManagerServiceImpl {
    async fn load_component(
        &self,
        request: Request<LoadComponentRequest>,
    ) -> Result<Response<LoadComponentResponse>, Status> {
        let req = request.into_inner();
        self.manager.load_component(&req.id, &req.path)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(LoadComponentResponse { 
            status: format!("component loaded: {}", req.id) 
        }))
    }

    async fn unload_component(
        &self,
        request: Request<UnloadComponentRequest>,
    ) -> Result<Response<UnloadComponentResponse>, Status> {
        let req = request.into_inner();
        self.manager.unload_component(&req.id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(UnloadComponentResponse { 
            status: format!("component unloaded: {}", req.id) 
        }))
    }

    async fn get_component(
        &self,
        request: Request<GetComponentRequest>,
    ) -> Result<Response<GetComponentResponse>, Status> {
        let req = request.into_inner();
        let component = self.manager.get_component(
            if req.id.is_empty() { None } else { Some(&req.id) }
        ).await.map_err(|e| Status::not_found(e.to_string()))?;
        
        let schema = component_exports_to_json_schema(&component, self.manager.engine.as_ref(), true);
        Ok(Response::new(GetComponentResponse { 
            id: req.id.clone(),
            details: serde_json::to_string(&schema).unwrap_or_default(),
        }))
    }

    async fn list_components(
        &self,
        _request: Request<ListComponentsRequest>,
    ) -> Result<Response<ListComponentsResponse>, Status> {
       let ids = self.manager.list_components().await;
        Ok(Response::new(ListComponentsResponse { ids }))
    }
}

pub struct WasmtimeD {
    addr: String,
    manager: Arc<LifecycleManager>,
}

impl WasmtimeD {
    pub fn new(addr: String) -> Result<Self, Box<dyn std::error::Error>> {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.async_support(true);
        let engine = Arc::new(wasmtime::Engine::new(&config)?);

        let manager = Arc::new(LifecycleManager::new(engine));

        Ok(Self { addr, manager })
    }

    pub async fn serve(self) -> Result<(), Box<dyn std::error::Error>> {
        let addr = self.addr.parse()?;
        let svc = LifecycleManagerServiceImpl { 
            manager: self.manager 
        };

        tracing::info!("Daemon listening on {}", addr);
        Server::builder()
            .add_service(LifecycleManagerServiceServer::new(svc))
            .serve(addr)
            .await?;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_max_level(tracing::Level::INFO).init();
    
    let daemon = WasmtimeD::new("[::1]:50051".to_string())?;
    daemon.serve().await
}
