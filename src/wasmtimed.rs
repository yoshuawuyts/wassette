use std::collections::HashMap;
use std::env;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
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
use policy_mcp::{PolicyDocument, PolicyParser};
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use wasmtime::component::Linker;
use wasmtime::Store;
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi_config::{WasiConfig, WasiConfigVariables};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpView};

const PATH_NOT_FILE_ERROR: &str = "Path is not a file";

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

#[derive(Clone)]
struct PreopenedDir {
    host_path: PathBuf,
    guest_path: String,
    dir_perms: wasmtime_wasi::DirPerms,
    file_perms: wasmtime_wasi::FilePerms,
}

#[derive(Default, Clone)]
struct NetworkPermissions {
    allow_tcp: bool,
    allow_udp: bool,
    allow_ip_name_lookup: bool,
}

#[derive(Clone)]
struct WasiStateTemplate {
    allow_stdout: bool,
    allow_stderr: bool,
    allow_args: bool,
    network_perms: NetworkPermissions,
    config_vars: HashMap<String, String>,
    preopened_dirs: Vec<PreopenedDir>,
}

impl Default for WasiStateTemplate {
    fn default() -> Self {
        // TODO: this is kind of permissive and we should review carefully what permissiosn to be granted by default
        Self {
            allow_stdout: true,
            allow_stderr: true,
            allow_args: true,
            network_perms: NetworkPermissions::default(),
            config_vars: HashMap::new(),
            preopened_dirs: Vec::new(),
        }
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
    plugin_dir: PathBuf,
    wasi_state_template: WasiStateTemplate,
}

fn create_wasi_state_template_from_policy(
    policy: &PolicyDocument,
    plugin_dir: &Path,
) -> anyhow::Result<WasiStateTemplate> {
    let env_vars = extract_env_vars(policy)?;
    let network_perms = extract_network_perms(policy);
    let preopened_dirs = extract_storage_permissions(policy, plugin_dir)?;

    Ok(WasiStateTemplate {
        network_perms,
        config_vars: env_vars,
        preopened_dirs,
        ..Default::default()
    })
}

fn extract_env_vars_virtual<F>(
    policy: &PolicyDocument,
    provider: F,
) -> anyhow::Result<HashMap<String, String>>
where
    F: Fn(&str) -> Result<String, env::VarError>,
{
    let mut env_vars = HashMap::new();
    if let Some(env_perms) = &policy.permissions.environment {
        if let Some(env_allow_vec) = &env_perms.allow {
            for env_allow in env_allow_vec {
                if let Ok(value) = provider(&env_allow.key) {
                    env_vars.insert(env_allow.key.clone(), value);
                }
            }
        }
    }
    Ok(env_vars)
}

fn extract_env_vars(policy: &PolicyDocument) -> anyhow::Result<HashMap<String, String>> {
    extract_env_vars_virtual(policy, |key| env::var(key))
}

fn extract_network_perms(policy: &PolicyDocument) -> NetworkPermissions {
    if let Some(network_perms) = &policy.permissions.network {
        let has_network_perms =
            network_perms.allow.is_some() && !network_perms.allow.as_ref().unwrap().is_empty();
        NetworkPermissions {
            allow_tcp: has_network_perms,
            allow_udp: has_network_perms,
            allow_ip_name_lookup: has_network_perms,
        }
    } else {
        NetworkPermissions::default()
    }
}

fn extract_storage_permissions(
    policy: &PolicyDocument,
    plugin_dir: &Path,
) -> anyhow::Result<Vec<PreopenedDir>> {
    let mut preopened_dirs = Vec::new();
    if let Some(storage) = &policy.permissions.storage {
        if let Some(allow) = &storage.allow {
            for storage_permission in allow {
                if storage_permission.uri.starts_with("fs://") {
                    let uri = storage_permission.uri.strip_prefix("fs://").unwrap();
                    let path = Path::new(uri);
                    let (file_perms, dir_perms) = calculate_permissions(&storage_permission.access);
                    let guest_path = path.to_string_lossy().to_string();
                    let host_path = plugin_dir.join(path);
                    preopened_dirs.push(PreopenedDir {
                        host_path,
                        guest_path,
                        dir_perms,
                        file_perms,
                    });
                }
            }
        }
    }
    Ok(preopened_dirs)
}

fn calculate_permissions(
    access_types: &[policy_mcp::AccessType],
) -> (wasmtime_wasi::FilePerms, wasmtime_wasi::DirPerms) {
    let file_perms = access_types
        .iter()
        .fold(wasmtime_wasi::FilePerms::empty(), |acc, access| {
            acc | match access {
                policy_mcp::AccessType::Read => wasmtime_wasi::FilePerms::READ,
                policy_mcp::AccessType::Write => wasmtime_wasi::FilePerms::WRITE,
            }
        });

    let dir_perms = access_types
        .iter()
        .fold(wasmtime_wasi::DirPerms::empty(), |acc, access| {
            acc | match access {
                policy_mcp::AccessType::Read => wasmtime_wasi::DirPerms::READ,
                policy_mcp::AccessType::Write => {
                    wasmtime_wasi::DirPerms::READ | wasmtime_wasi::DirPerms::MUTATE
                }
            }
        });

    (file_perms, dir_perms)
}

impl LifecycleManagerServiceImpl {
    fn new(
        manager: Arc<LifecycleManager>,
        plugin_dir: PathBuf,
        policy_file: Option<&str>,
    ) -> anyhow::Result<Self> {
        let wasi_state_template = if let Some(policy_path) = policy_file {
            let policy = PolicyParser::parse_file(policy_path)?;
            create_wasi_state_template_from_policy(&policy, &plugin_dir)?
        } else {
            WasiStateTemplate::default()
        };

        Ok(Self {
            manager,
            plugin_dir,
            wasi_state_template,
        })
    }

    async fn copy_to_dir(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let metadata = tokio::fs::metadata(&path).await?;
        if !metadata.is_file() {
            return Err(std::io::Error::other(PATH_NOT_FILE_ERROR));
        }
        // NOTE: We just checked this was a file by reading metadata so we can unwrap here
        let dest = self.plugin_dir.join(path.as_ref().file_name().unwrap());
        tokio::fs::copy(path, dest).await.map(|_| ())
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
        // Load the request file into the directory
        if let Err(e) = self.copy_to_dir(&req.path).await {
            let status = match e.kind() {
                ErrorKind::NotFound => {
                    Status::invalid_argument(format!("No file found at path {}", req.path))
                }
                ErrorKind::Other if e.to_string().contains(PATH_NOT_FILE_ERROR) => {
                    Status::invalid_argument(format!("Given path {} is not a file", req.path))
                }
                _ => Status::internal(e.to_string()),
            };
            return Err(status);
        }
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
    plugin_dir: PathBuf,
    policy_file: Option<String>,
}

impl WasmtimeD {
    pub async fn new(
        addr: String,
        plugin_dir: impl AsRef<Path>,
        policy_file: Option<&str>,
    ) -> anyhow::Result<Self> {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.async_support(true);
        let engine = Arc::new(wasmtime::Engine::new(&config)?);

        let manager = Arc::new(LifecycleManager::new(engine, &plugin_dir).await?);

        Ok(Self {
            addr,
            manager,
            plugin_dir: plugin_dir.as_ref().to_path_buf(),
            policy_file: policy_file.map(|s| s.to_string()),
        })
    }

    pub async fn serve(self) -> anyhow::Result<()> {
        let addr = self.addr.parse()?;
        let svc = LifecycleManagerServiceImpl::new(
            self.manager,
            self.plugin_dir,
            self.policy_file.as_deref(),
        )?;

        tracing::info!("Daemon listening on {}", addr);
        Server::builder()
            .add_service(LifecycleManagerServiceServer::new(svc))
            .serve(addr)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use policy_mcp::AccessType;
    use tempfile::TempDir;

    use super::*;

    fn create_test_policy() -> PolicyDocument {
        let yaml_content = r#"
version: "1.0"
description: "Test policy for mcp-wasmtime"
permissions:
  network:
    allow:
      - host: "api.example.com"
  environment:
    allow:
      - key: "TEST_VAR"
      - key: "NONEXISTENT_VAR"
  storage:
    allow:
      - uri: "fs://test/path"
        access: ["read"]
      - uri: "fs://write/path"
        access: ["write"]
      - uri: "fs://readwrite/path"
        access: ["read", "write"]
      - uri: "http://not-fs"
        access: ["read"]
"#;
        PolicyParser::parse_str(yaml_content).unwrap()
    }

    #[test]
    fn test_calculate_permissions_read_only() {
        let access_types = vec![AccessType::Read];
        let (file_perms, dir_perms) = calculate_permissions(&access_types);

        assert_eq!(file_perms, wasmtime_wasi::FilePerms::READ);
        assert_eq!(dir_perms, wasmtime_wasi::DirPerms::READ);
    }

    #[test]
    fn test_calculate_permissions_write_only() {
        let access_types = vec![AccessType::Write];
        let (file_perms, dir_perms) = calculate_permissions(&access_types);

        assert_eq!(file_perms, wasmtime_wasi::FilePerms::WRITE);
        assert_eq!(
            dir_perms,
            wasmtime_wasi::DirPerms::READ | wasmtime_wasi::DirPerms::MUTATE
        );
    }

    #[test]
    fn test_calculate_permissions_read_write() {
        let access_types = vec![AccessType::Read, AccessType::Write];
        let (file_perms, dir_perms) = calculate_permissions(&access_types);

        assert_eq!(
            file_perms,
            wasmtime_wasi::FilePerms::READ | wasmtime_wasi::FilePerms::WRITE
        );
        assert_eq!(
            dir_perms,
            wasmtime_wasi::DirPerms::READ | wasmtime_wasi::DirPerms::MUTATE
        );
    }

    #[test]
    fn test_calculate_permissions_empty() {
        let access_types = vec![];
        let (file_perms, dir_perms) = calculate_permissions(&access_types);

        assert_eq!(file_perms, wasmtime_wasi::FilePerms::empty());
        assert_eq!(dir_perms, wasmtime_wasi::DirPerms::empty());
    }

    #[test]
    fn test_extract_environment_variables_from_policy() {
        let policy = create_test_policy();

        // Mock environment provider that returns specific values for testing
        let mock_env = |key: &str| -> Result<String, env::VarError> {
            match key {
                "TEST_VAR" => Ok("test_value".to_string()),
                "NONEXISTENT_VAR" => Err(env::VarError::NotPresent),
                _ => Err(env::VarError::NotPresent),
            }
        };

        let env_vars = extract_env_vars_virtual(&policy, mock_env).unwrap();

        assert_eq!(env_vars.get("TEST_VAR"), Some(&"test_value".to_string()));
        assert!(!env_vars.contains_key("NONEXISTENT_VAR"));
    }

    #[test]
    fn test_extract_network_permissions_with_allow() {
        let policy = create_test_policy();
        let network_perms = extract_network_perms(&policy);

        assert!(network_perms.allow_tcp);
        assert!(network_perms.allow_udp);
        assert!(network_perms.allow_ip_name_lookup);
    }

    #[test]
    fn test_extract_storage_permissions() {
        let temp_dir = TempDir::new().unwrap();
        let plugin_dir = temp_dir.path();

        let policy = create_test_policy();
        let preopened_dirs = extract_storage_permissions(&policy, plugin_dir).unwrap();

        assert_eq!(preopened_dirs.len(), 3);

        let read_only = &preopened_dirs[0];
        assert_eq!(read_only.guest_path, "test/path");
        assert_eq!(read_only.host_path, plugin_dir.join("test/path"));
        assert_eq!(read_only.file_perms, wasmtime_wasi::FilePerms::READ);
        assert_eq!(read_only.dir_perms, wasmtime_wasi::DirPerms::READ);

        let write_only = &preopened_dirs[1];
        assert_eq!(write_only.guest_path, "write/path");
        assert_eq!(write_only.file_perms, wasmtime_wasi::FilePerms::WRITE);
        assert_eq!(
            write_only.dir_perms,
            wasmtime_wasi::DirPerms::READ | wasmtime_wasi::DirPerms::MUTATE
        );

        let read_write = &preopened_dirs[2];
        assert_eq!(read_write.guest_path, "readwrite/path");
        assert_eq!(
            read_write.file_perms,
            wasmtime_wasi::FilePerms::READ | wasmtime_wasi::FilePerms::WRITE
        );
        assert_eq!(
            read_write.dir_perms,
            wasmtime_wasi::DirPerms::READ | wasmtime_wasi::DirPerms::MUTATE
        );
    }

    #[test]
    fn test_create_wasi_state_template_from_policy() {
        let temp_dir = TempDir::new().unwrap();
        let plugin_dir = temp_dir.path();
        let policy = create_test_policy();

        let template = create_wasi_state_template_from_policy(&policy, plugin_dir).unwrap();

        assert!(template.network_perms.allow_tcp);
        assert!(template.network_perms.allow_udp);
        assert!(template.network_perms.allow_ip_name_lookup);
        // Note: This test doesn't check env vars since it uses the real env::var function
        // For isolated env var testing, see test_extract_environment_variables_from_policy
        assert_eq!(template.preopened_dirs.len(), 3);
    }
}
