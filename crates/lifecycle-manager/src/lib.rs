use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use futures::TryStreamExt;
use serde_json::Value;
use tokio::fs::DirEntry;
use tokio::sync::RwLock;
use tracing::{debug, error, info, instrument};
use wasmtime::component::Component;
use wasmtime::Engine;

#[derive(Debug, Clone)]
struct ToolInfo {
    component_id: String,
    schema: Value,
}

#[derive(Debug, Default)]
pub struct ComponentRegistry {
    tool_map: HashMap<String, Vec<ToolInfo>>,
    component_map: HashMap<String, Vec<String>>,
}

/// The returned status when loading a component
#[derive(Debug, PartialEq)]
pub enum LoadResult {
    /// Indicates that the component was loaded but replaced a currently loaded component
    Replaced,
    /// Indicates that the component did not exist and is now loaded
    New,
}

impl ComponentRegistry {
    fn new() -> Self {
        Self::default()
    }

    fn register_component(&mut self, component_id: &str, schema: &Value) -> Result<()> {
        let tools = schema["tools"]
            .as_array()
            .context("Schema does not contain tools array")?;

        let mut component_tools = Vec::new();

        for tool in tools {
            let name = tool["name"]
                .as_str()
                .context("Tool name is not a string")?
                .to_string();

            let tool_info = ToolInfo {
                component_id: component_id.to_string(),
                schema: tool.clone(),
            };

            self.tool_map
                .entry(name.clone())
                .or_default()
                .push(tool_info);

            component_tools.push(name);
        }

        self.component_map
            .insert(component_id.to_string(), component_tools);
        Ok(())
    }

    fn unregister_component(&mut self, component_id: &str) {
        if let Some(tools) = self.component_map.remove(component_id) {
            for tool_name in tools {
                if let Some(tool_infos) = self.tool_map.get_mut(&tool_name) {
                    tool_infos.retain(|info| info.component_id != component_id);
                    if tool_infos.is_empty() {
                        self.tool_map.remove(&tool_name);
                    }
                }
            }
        }
    }

    fn get_tool_info(&self, tool_name: &str) -> Option<&Vec<ToolInfo>> {
        self.tool_map.get(tool_name)
    }

    fn list_tools(&self) -> Vec<Value> {
        self.tool_map
            .values()
            .flat_map(|tools| tools.iter().map(|t| t.schema.clone()))
            .collect()
    }
}

/// A manager that handles the dynamic lifecycle of WebAssembly components.
pub struct LifecycleManager {
    pub engine: Arc<Engine>,
    pub components: Arc<RwLock<HashMap<String, Arc<Component>>>>,
    pub registry: Arc<RwLock<ComponentRegistry>>,
}

impl LifecycleManager {
    /// Creates a lifecycle manager, loading the current components from the given plugin directory
    #[instrument(skip(engine), fields(plugin_dir = %plugin_dir.as_ref().display()))]
    pub async fn new(engine: Arc<Engine>, plugin_dir: impl AsRef<Path>) -> Result<Self> {
        info!("Creating new LifecycleManager");

        let mut registry = ComponentRegistry::new();
        let mut components = HashMap::new();

        let loaded_components =
            tokio_stream::wrappers::ReadDirStream::new(tokio::fs::read_dir(plugin_dir).await?)
                .map_err(anyhow::Error::from)
                .try_filter_map(|entry| {
                    let value = engine.clone();
                    async move { load_component_from_entry(value, entry).await }
                })
                .try_collect::<Vec<_>>()
                .await?;

        for (component, name) in loaded_components.into_iter() {
            let schema =
                component2json::component_exports_to_json_schema(&component, &engine, true);
            registry
                .register_component(&name, &schema)
                .context("unable to insert component into registry")?;
            components.insert(name, Arc::new(component));
        }

        info!("LifecycleManager initialized successfully");
        Ok(Self {
            engine,
            components: Arc::new(RwLock::new(components)),
            registry: Arc::new(RwLock::new(registry)),
        })
    }

    /// Loads a new component from a local path using the file stem as the ID of the component
    ///
    /// If a component with the given id already exists, it will be updated with the new component.
    /// Returns the new ID and whether or not this component was replaced.
    #[instrument(skip(self), fields(path = %path.as_ref().display()))]
    pub async fn load_component(&self, path: impl AsRef<Path>) -> Result<(String, LoadResult)> {
        debug!("Loading component from path");

        // Validate path exists
        if !tokio::fs::try_exists(path.as_ref()).await? {
            error!("Component path does not exist: {}", path.as_ref().display());
            bail!("Component path does not exist: {}. Please provide a valid path to a WebAssembly component file.", path.as_ref().display());
        }

        // Validate file extension
        if path.as_ref().extension().unwrap_or_default() != "wasm" {
            error!("Invalid file extension for component");
            bail!("Invalid file extension for component: {}. Component file must have .wasm extension.", path.as_ref().display());
        }

        let component = match Component::from_file(&self.engine, &path) {
            Ok(comp) => comp,
            Err(e) => {
                error!(err = ?e, "Failed to load component from path");
                bail!("Failed to load component from path: {}. Error: {}. Please ensure the file is a valid WebAssembly component.", path.as_ref().display(), e);
            }
        };

        let id = path
            .as_ref()
            .file_stem()
            .context("Couldn't find file name")?
            .to_str()
            .context("File name was not a valid string")?
            .to_string();

        let schema =
            component2json::component_exports_to_json_schema(&component, &self.engine, true);

        {
            let mut registry_write = self.registry.write().await;
            registry_write.unregister_component(&id);
            registry_write.register_component(&id, &schema)?;
        }

        let res = self
            .components
            .write()
            .await
            .insert(id.clone(), Arc::new(component))
            .map(|_| LoadResult::Replaced)
            .unwrap_or(LoadResult::New);

        info!("Successfully loaded component");
        Ok((id, res))
    }

    /// Unloads the component with the specified id.
    #[instrument(skip(self))]
    pub async fn unload_component(&self, id: &str) {
        debug!("Unloading component");
        self.components.write().await.remove(id);
        self.registry.write().await.unregister_component(id);
    }

    /// Returns the component ID for a given tool name.
    /// If there are multiple components with the same tool name, returns an error.
    #[instrument(skip(self))]
    pub async fn get_component_id_for_tool(&self, tool_name: &str) -> Result<String> {
        let registry = self.registry.read().await;
        let tool_infos = registry
            .get_tool_info(tool_name)
            .context("Tool not found")?;

        if tool_infos.len() > 1 {
            bail!(
                "Multiple components found for tool '{}': {}",
                tool_name,
                tool_infos
                    .iter()
                    .map(|info| info.component_id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        Ok(tool_infos[0].component_id.clone())
    }

    /// Lists all available tools across all components
    #[instrument(skip(self))]
    pub async fn list_tools(&self) -> Vec<Value> {
        self.registry.read().await.list_tools()
    }

    /// Returns the requested component. Returns `None` if the component is not found.
    #[instrument(skip(self))]
    pub async fn get_component(&self, component_id: &str) -> Option<Arc<Component>> {
        self.components.read().await.get(component_id).cloned()
    }

    #[instrument(skip(self))]
    pub async fn list_components(&self) -> Vec<String> {
        self.components.read().await.keys().cloned().collect()
    }
}

async fn load_component_from_entry(
    engine: Arc<Engine>,
    entry: DirEntry,
) -> Result<Option<(Component, String)>> {
    let is_file = entry
        .metadata()
        .await
        .map(|m| m.is_file())
        .context("unable to read file metadata")?;
    let is_wasm = entry
        .path()
        .extension()
        .map(|ext| ext == "wasm")
        .unwrap_or(false);
    if !(is_file && is_wasm) {
        return Ok(None);
    }
    let entry_path = entry.path();
    let component =
        tokio::task::spawn_blocking(move || Component::from_file(&engine, entry_path)).await??;
    let name = entry
        .path()
        .file_stem()
        .and_then(|s| s.to_str())
        .map(String::from)
        .context("wasm file didn't have a valid file name")?;
    Ok(Some((component, name)))
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;
    use std::path::PathBuf;
    use std::process::Command;

    use serde_json::json;
    use test_log::test;

    use super::*;

    const TEST_COMPONENT_ID: &str = "fetch_rs";

    /// Helper struct for keeping a reference to the temporary directory used for testing the
    /// lifecycle manager
    struct TestLifecycleManager {
        pub manager: LifecycleManager,
        _tempdir: tempfile::TempDir,
    }

    impl TestLifecycleManager {
        pub async fn load_test_component(&self) -> Result<()> {
            let component_path = build_example_component().await?;

            self.manager.load_component(&component_path).await?;

            Ok(())
        }
    }

    impl Deref for TestLifecycleManager {
        type Target = LifecycleManager;

        fn deref(&self) -> &Self::Target {
            &self.manager
        }
    }

    async fn create_test_manager() -> Result<TestLifecycleManager> {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.async_support(true);
        let engine = Arc::new(wasmtime::Engine::new(&config)?);
        let tempdir = tempfile::tempdir()?;
        let manager = LifecycleManager::new(engine, &tempdir).await?;
        Ok(TestLifecycleManager {
            manager,
            _tempdir: tempdir,
        })
    }

    async fn build_example_component() -> Result<PathBuf> {
        let cwd = std::env::current_dir()?;
        println!("CWD: {}", cwd.display());
        let component_path =
            cwd.join("../../examples/fetch-rs/target/wasm32-wasip2/release/fetch_rs.wasm");

        if !component_path.exists() {
            let status = Command::new("cargo")
                .current_dir(cwd.join("../../examples/fetch-rs"))
                .args(["build", "--release", "--target", "wasm32-wasip2"])
                .status()
                .context("Failed to execute cargo component build")?;

            if !status.success() {
                anyhow::bail!("Failed to compile fetch-rs component");
            }
        }

        if !component_path.exists() {
            anyhow::bail!(
                "Component file not found after build: {}",
                component_path.display()
            );
        }

        Ok(component_path)
    }

    #[test]
    fn test_component_registry() {
        let mut registry = ComponentRegistry::new();

        // Test registering a component with tools
        let schema = json!({
            "tools": [
                {
                    "name": "tool1",
                    "description": "Test tool 1"
                },
                {
                    "name": "tool2",
                    "description": "Test tool 2"
                }
            ]
        });

        registry.register_component("comp1", &schema).unwrap();

        // Test tool lookup
        let tool1_info = registry.get_tool_info("tool1").unwrap();
        assert_eq!(tool1_info[0].component_id, "comp1");

        // Test listing tools
        let tools = registry.list_tools();
        assert_eq!(tools.len(), 2);

        // Test registering another component with overlapping tool name
        let schema2 = json!({
            "tools": [
                {
                    "name": "tool1",
                    "description": "Test tool 1 from comp2"
                }
            ]
        });

        registry.register_component("comp2", &schema2).unwrap();

        // Verify tool1 now has two components
        let tool1_info = registry.get_tool_info("tool1").unwrap();
        assert_eq!(tool1_info.len(), 2);

        // Test unregistering a component
        registry.unregister_component("comp1");

        // Verify tool2 is gone and tool1 only has one component
        assert!(registry.get_tool_info("tool2").is_none());
        let tool1_info = registry.get_tool_info("tool1").unwrap();
        assert_eq!(tool1_info.len(), 1);
        assert_eq!(tool1_info[0].component_id, "comp2");
    }

    #[test(tokio::test)]
    async fn test_lifecycle_manager_tool_registry() -> Result<()> {
        let manager = create_test_manager().await?;

        let temp_dir = tempfile::tempdir()?;
        let component_path = temp_dir.path().join("mock_component.wasm");
        std::fs::write(&component_path, b"mock wasm bytes")?;

        let load_result = manager.load_component(component_path).await;
        assert!(load_result.is_err()); // Expected since we're using invalid WASM

        let lookup_result = manager.get_component_id_for_tool("non-existent").await;
        assert!(lookup_result.is_err());

        Ok(())
    }

    #[test(tokio::test)]
    async fn test_new_manager() -> Result<()> {
        let _manager = create_test_manager().await?;
        Ok(())
    }

    #[test(tokio::test)]
    async fn test_load_and_unload_component() -> Result<()> {
        let manager = create_test_manager().await?;

        let load_result = manager.load_component("/path/to/nonexistent").await;
        assert!(load_result.is_err());

        manager.load_test_component().await?;

        let loaded_components = manager.list_components().await;
        assert_eq!(loaded_components.len(), 1);

        manager.unload_component(TEST_COMPONENT_ID).await;

        let loaded_components = manager.list_components().await;
        assert!(loaded_components.is_empty());

        Ok(())
    }

    #[test(tokio::test)]
    async fn test_get_component() -> Result<()> {
        let manager = create_test_manager().await?;
        assert!(manager.get_component("non-existent").await.is_none());

        manager.load_test_component().await?;

        manager
            .get_component(TEST_COMPONENT_ID)
            .await
            .expect("Should be able to get a component we just loaded");
        Ok(())
    }

    #[test(tokio::test)]
    async fn test_duplicate_component_id() -> Result<()> {
        let manager = create_test_manager().await?;

        manager.load_test_component().await?;

        let components = manager.list_components().await;
        assert_eq!(components.len(), 1);
        assert_eq!(components[0], TEST_COMPONENT_ID);

        // Load again and make sure we still only have one

        manager.load_test_component().await?;
        let components = manager.list_components().await;
        assert_eq!(components.len(), 1);
        assert_eq!(components[0], TEST_COMPONENT_ID);

        Ok(())
    }

    #[test(tokio::test)]
    async fn test_component_reload() -> Result<()> {
        let manager = create_test_manager().await?;
        let component_path = build_example_component().await?;

        manager
            .load_component(component_path.to_str().unwrap())
            .await?;

        let component_id = manager.get_component_id_for_tool("fetch").await?;
        assert_eq!(component_id, TEST_COMPONENT_ID);

        manager
            .load_component(component_path.to_str().unwrap())
            .await?;

        let component_id = manager.get_component_id_for_tool("fetch").await?;
        assert_eq!(component_id, TEST_COMPONENT_ID);

        Ok(())
    }

    #[test(tokio::test)]
    async fn test_component_path_update() -> Result<()> {
        let manager = create_test_manager().await?;
        let component_path = build_example_component().await?;

        let (_, res) = manager
            .load_component(component_path.to_str().unwrap())
            .await?;

        assert_eq!(LoadResult::New, res, "Should have been a new component");

        let (_, res) = manager
            .load_component(component_path.to_str().unwrap())
            .await?;

        assert_eq!(
            LoadResult::Replaced,
            res,
            "Should have replaced the component"
        );

        let component_id = manager.get_component_id_for_tool("fetch").await?;
        assert_eq!(component_id, TEST_COMPONENT_ID);

        Ok(())
    }
}
