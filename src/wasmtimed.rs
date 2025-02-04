use std::sync::Arc;

use component2json::{component_exports_to_json_schema, json_to_vals, vals_to_json};
use lifecycle_manager::LifecycleManager;
use lifecycle_proto::lifecycle::lifecycle_manager_service_server::{
    LifecycleManagerService, LifecycleManagerServiceServer,
};
use lifecycle_proto::lifecycle::{
    CallComponentRequest, CallComponentResponse, GetComponentRequest, GetComponentResponse,
    ListComponentsRequest, ListComponentsResponse, LoadComponentRequest, LoadComponentResponse,
    UnloadComponentRequest, UnloadComponentResponse,
};
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use wasmtime::component::Linker;
use wasmtime::Store;
use wasmtime_wasi::WasiCtxBuilder;

struct WasiState {
    ctx: wasmtime_wasi::WasiCtx,
    table: wasmtime_wasi::ResourceTable,
}

impl wasmtime_wasi::WasiView for WasiState {
    fn table(&mut self) -> &mut wasmtime_wasi::ResourceTable {
        &mut self.table
    }

    fn ctx(&mut self) -> &mut wasmtime_wasi::WasiCtx {
        &mut self.ctx
    }
}

#[derive(Clone)]
struct LifecycleManagerServiceImpl {
    manager: Arc<LifecycleManager>,
}

impl LifecycleManagerServiceImpl {
    async fn execute_component_call(
        &self,
        component: &wasmtime::component::Component,
        function_name: &str,
        parameters: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let mut linker = Linker::new(self.manager.engine.as_ref());
        wasmtime_wasi::add_to_linker_async(&mut linker)?;

        let table = wasmtime_wasi::ResourceTable::default();
        let ctx = WasiCtxBuilder::new()
            .inherit_stdio()
            .inherit_args()
            .inherit_env()
            .build();

        let state = WasiState { ctx, table };
        let mut store = Store::new(self.manager.engine.as_ref(), state);

        let instance = linker.instantiate_async(&mut store, component).await?;

        let params: serde_json::Value = serde_json::from_str(parameters)?;
        let argument_vals = json_to_vals(&params)?;

        let export = instance
            .get_export(&mut store, None, function_name)
            .ok_or_else(|| anyhow::anyhow!("Function not found: {}", function_name))?;

        let func = instance
            .get_func(&mut store, export)
            .ok_or_else(|| anyhow::anyhow!("Export is not a function: {}", function_name))?;

        let schema =
            component_exports_to_json_schema(component, self.manager.engine.as_ref(), true);
        let tools = schema
            .get("tools")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("No tools found in component"))?;

        let tool = tools
            .iter()
            .find(|t| t.get("name").and_then(|n| n.as_str()) == Some(function_name))
            .ok_or_else(|| anyhow::anyhow!("Tool not found"))?;

        let output_schema = tool["outputSchema"].clone();
        let mut results = json_to_vals(&output_schema)?;

        func.call_async(&mut store, &argument_vals, &mut results)
            .await?;

        let result_json = vals_to_json(&results);
        Ok(serde_json::to_string(&result_json)?)
    }
}

#[tonic::async_trait]
impl LifecycleManagerService for LifecycleManagerServiceImpl {
    async fn load_component(
        &self,
        request: Request<LoadComponentRequest>,
    ) -> Result<Response<LoadComponentResponse>, Status> {
        let req = request.into_inner();
        self.manager
            .load_component(&req.id, &req.path)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(LoadComponentResponse {
            status: format!("component loaded: {}", req.id),
        }))
    }

    async fn unload_component(
        &self,
        request: Request<UnloadComponentRequest>,
    ) -> Result<Response<UnloadComponentResponse>, Status> {
        let req = request.into_inner();
        self.manager
            .unload_component(&req.id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(UnloadComponentResponse {
            status: format!("component unloaded: {}", req.id),
        }))
    }

    async fn get_component(
        &self,
        request: Request<GetComponentRequest>,
    ) -> Result<Response<GetComponentResponse>, Status> {
        let req = request.into_inner();
        let component = self
            .manager
            .get_component(if req.id.is_empty() {
                None
            } else {
                Some(&req.id)
            })
            .await
            .map_err(|e| Status::not_found(e.to_string()))?;

        let schema =
            component_exports_to_json_schema(&component, self.manager.engine.as_ref(), true);
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

    async fn call_component(
        &self,
        request: Request<CallComponentRequest>,
    ) -> Result<Response<CallComponentResponse>, Status> {
        let req = request.into_inner();

        let component = self
            .manager
            .get_component(Some(&req.id))
            .await
            .map_err(|e| Status::not_found(e.to_string()))?;

        match self
            .execute_component_call(&component, &req.function_name, &req.parameters)
            .await
        {
            Ok(result) => Ok(Response::new(CallComponentResponse {
                result: result.into_bytes(),
                error: String::new(),
            })),
            Err(e) => Ok(Response::new(CallComponentResponse {
                result: Vec::new(),
                error: e.to_string(),
            })),
        }
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
            manager: self.manager,
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
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let daemon = WasmtimeD::new("[::1]:50051".to_string())?;
    daemon.serve().await
}
