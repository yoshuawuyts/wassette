use std::path::Path;
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
use policy_mcp::PolicyParser;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use wasmtime::component::Linker;
use wasmtime::Store;
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi_config::{WasiConfig, WasiConfigVariables};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpView};

mod wasistate;
use wasistate::{create_wasi_state_template_from_policy, WasiStateTemplate};

struct WasiState {
    ctx: wasmtime_wasi::WasiCtx,
    table: wasmtime_wasi::ResourceTable,
    http: wasmtime_wasi_http::WasiHttpCtx,
    wasi_config_vars: WasiConfigVariables,
}

impl wasmtime_wasi::IoView for WasiState {
    fn table(&mut self) -> &mut wasmtime_wasi::ResourceTable {
        &mut self.table
    }
}

impl wasmtime_wasi::WasiView for WasiState {
    fn ctx(&mut self) -> &mut wasmtime_wasi::WasiCtx {
        &mut self.ctx
    }
}

impl WasiHttpView for WasiState {
    fn ctx(&mut self) -> &mut WasiHttpCtx {
        &mut self.http
    }
}

impl WasiStateTemplate {
    fn build(&self) -> anyhow::Result<WasiState> {
        let mut ctx_builder = WasiCtxBuilder::new();
        if self.allow_stdout {
            ctx_builder.inherit_stdout();
        }
        if self.allow_stderr {
            ctx_builder.inherit_stderr();
        }
        ctx_builder.inherit_args();
        if self.allow_args {
            ctx_builder.inherit_args();
        }
        ctx_builder.inherit_network();
        ctx_builder.allow_tcp(self.network_perms.allow_tcp);
        ctx_builder.allow_udp(self.network_perms.allow_udp);
        ctx_builder.allow_ip_name_lookup(self.network_perms.allow_ip_name_lookup);
        for preopened_dir in &self.preopened_dirs {
            ctx_builder.preopened_dir(
                preopened_dir.host_path.as_path(),
                preopened_dir.guest_path.as_str(),
                preopened_dir.dir_perms,
                preopened_dir.file_perms,
            )?;
        }

        Ok(WasiState {
            ctx: ctx_builder.build(),
            table: wasmtime_wasi::ResourceTable::default(),
            http: WasiHttpCtx::new(),
            wasi_config_vars: WasiConfigVariables::from_iter(self.config_vars.clone()),
        })
    }
}

#[derive(Clone)]
struct LifecycleManagerServiceImpl {
    manager: Arc<LifecycleManager>,
    wasi_state_template: WasiStateTemplate,
}

impl LifecycleManagerServiceImpl {
    fn new(
        manager: Arc<LifecycleManager>,
        wasi_state_template: WasiStateTemplate,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            manager,
            wasi_state_template,
        })
    }

    async fn execute_component_call(
        &self,
        component: &wasmtime::component::Component,
        function_name: &str,
        parameters: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let mut linker = Linker::new(self.manager.engine.as_ref());
        wasmtime_wasi::add_to_linker_async(&mut linker)?;
        wasmtime_wasi_http::add_only_http_to_linker_async(&mut linker)?;
        wasmtime_wasi_config::add_to_linker(&mut linker, |h: &mut WasiState| {
            WasiConfig::from(&h.wasi_config_vars)
        })?;

        // Use the pre-built WASI state template
        let state = self.wasi_state_template.build()?;
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
        let (id, _) = self
            .manager
            .load_component(&req.path)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(LoadComponentResponse {
            status: "component loaded".to_string(),
            id,
        }))
    }

    async fn unload_component(
        &self,
        request: Request<UnloadComponentRequest>,
    ) -> Result<Response<UnloadComponentResponse>, Status> {
        let req = request.into_inner();
        self.manager.unload_component(&req.id).await;
        Ok(Response::new(UnloadComponentResponse {
            status: format!("component unloaded: {}", req.id),
        }))
    }

    async fn get_component(
        &self,
        request: Request<GetComponentRequest>,
    ) -> Result<Response<GetComponentResponse>, Status> {
        let req = request.into_inner();
        if req.id.is_empty() {
            return Err(Status::invalid_argument("ID field must be set"));
        }
        let component = self.manager.get_component(&req.id).await.ok_or_else(|| {
            Status::not_found(format!("Component with ID of {} not found", req.id))
        })?;

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

        let component_id = self
            .manager
            .get_component_id_for_tool(&req.function_name)
            .await
            .map_err(|e| {
                Status::not_found(format!(
                    "Failed to find component for tool '{}': {}",
                    req.function_name, e
                ))
            })?;

        let component = self
            .manager
            .get_component(&component_id)
            .await
            .ok_or_else(|| {
                Status::not_found(format!("Component with ID {} not found", component_id))
            })?;

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
    wasi_state_template: WasiStateTemplate,
}

impl WasmtimeD {
    pub async fn new(
        addr: String,
        plugin_dir: impl AsRef<Path>,
        policy_file: Option<&str>,
    ) -> anyhow::Result<Self> {
        Self::new_with_clients(
            addr,
            plugin_dir,
            policy_file,
            oci_client::Client::default(),
            reqwest::Client::default(),
        )
        .await
    }

    /// Same as `new`, but allows passing in custom clients for OCI and HTTP.
    // This is mostly for testing purposes, but if we export publicly would be useful as well
    #[allow(dead_code)]
    pub async fn new_with_clients(
        addr: String,
        plugin_dir: impl AsRef<Path>,
        policy_file: Option<&str>,
        oci_cli: oci_client::Client,
        http_client: reqwest::Client,
    ) -> anyhow::Result<Self> {
        let wasi_state_template = if let Some(policy_path) = policy_file {
            let policy = PolicyParser::parse_file(policy_path)?;
            create_wasi_state_template_from_policy(&policy, plugin_dir.as_ref())?
        } else {
            WasiStateTemplate::default()
        };
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.async_support(true);
        let engine = Arc::new(wasmtime::Engine::new(&config)?);
        let manager = Arc::new(
            LifecycleManager::new_with_clients(engine, plugin_dir, oci_cli, http_client).await?,
        );

        Ok(Self {
            addr,
            manager,
            wasi_state_template,
        })
    }

    pub async fn serve(self) -> anyhow::Result<()> {
        let addr = self.addr.parse()?;
        let svc = LifecycleManagerServiceImpl::new(self.manager, self.wasi_state_template)?;

        tracing::info!("Daemon listening on {}", addr);
        Server::builder()
            .add_service(LifecycleManagerServiceServer::new(svc))
            .serve(addr)
            .await?;
        Ok(())
    }
}
