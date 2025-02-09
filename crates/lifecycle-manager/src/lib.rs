use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use serde_json::Value;
use sqlx::{Pool, Row, Sqlite, SqlitePool};
use tokio::sync::RwLock;
use tracing::{debug, info, instrument, warn};
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
    pub db: Pool<Sqlite>,
}

impl LifecycleManager {
    #[instrument(skip(engine))]
    pub async fn new(engine: Arc<Engine>) -> Result<Self> {
        Self::new_with_db_url(engine, "sqlite:components.db").await
    }

    #[instrument(skip(engine))]
    pub async fn new_with_db_url(engine: Arc<Engine>, db_url: &str) -> Result<Self> {
        info!("Creating new LifecycleManager");
        let db = SqlitePool::connect(db_url)
            .await
            .context("Failed to connect to SQLite database")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS components (
                id TEXT PRIMARY KEY,
                path TEXT NOT NULL
            )",
        )
        .execute(&db)
        .await
        .context("Failed to create components table")?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS tools (
                tool_name TEXT NOT NULL,
                component_id TEXT NOT NULL,
                schema TEXT NOT NULL,
                FOREIGN KEY(component_id) REFERENCES components(id) ON DELETE CASCADE,
                PRIMARY KEY(tool_name, component_id)
            )",
        )
        .execute(&db)
        .await
        .context("Failed to create tools table")?;

        let registry = Arc::new(RwLock::new(ComponentRegistry::new()));

        let tools = sqlx::query("SELECT tool_name, component_id, schema FROM tools")
            .fetch_all(&db)
            .await
            .context("Failed to load tools from database")?;

        let mut registry_write = registry.write().await;
        for tool in tools {
            let tool_name: String = tool.try_get("tool_name")?;
            let component_id: String = tool.try_get("component_id")?;
            let schema_str: String = tool.try_get("schema")?;
            let schema: Value = serde_json::from_str(&schema_str)
                .context("Failed to parse tool schema from database")?;

            registry_write
                .tool_map
                .entry(tool_name.clone())
                .or_default()
                .push(ToolInfo {
                    component_id: component_id.clone(),
                    schema,
                });

            registry_write
                .component_map
                .entry(component_id)
                .or_default()
                .push(tool_name);
        }
        drop(registry_write);

        info!("LifecycleManager initialized successfully");
        Ok(Self {
            engine,
            components: Arc::new(RwLock::new(HashMap::new())),
            registry,
            db,
        })
    }

    /// Loads a new component from a local path (or, in the future, from an OCI URL)
    /// and associates it with the given id. If a component with the given id already exists,
    /// it will be updated with the new component.
    #[instrument(skip(self))]
    pub async fn load_component(&self, id: &str, path: &str) -> Result<()> {
        debug!("Loading component from path");
        let component = Component::from_file(&self.engine, path)
            .with_context(|| format!("Failed to load component from path: {}", path))?;

        let schema =
            component2json::component_exports_to_json_schema(&component, &self.engine, true);

        let mut tx = self.db.begin().await?;

        self.registry.write().await.unregister_component(id);

        sqlx::query("INSERT OR REPLACE INTO components (id, path) VALUES (?, ?)")
            .bind(id)
            .bind(path)
            .execute(&mut *tx)
            .await
            .context("Failed to store component in database")?;

        let tools = schema["tools"]
            .as_array()
            .context("Schema does not contain tools array")?;

        sqlx::query("DELETE FROM tools WHERE component_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .context("Failed to remove old tools from database")?;

        for tool in tools {
            let name = tool["name"]
                .as_str()
                .context("Tool name is not a string")?
                .to_string();

            sqlx::query("INSERT INTO tools (tool_name, component_id, schema) VALUES (?, ?, ?)")
                .bind(&name)
                .bind(id)
                .bind(serde_json::to_string(tool)?)
                .execute(&mut *tx)
                .await
                .context("Failed to store tool in database")?;
        }

        tx.commit().await?;

        self.registry
            .write()
            .await
            .register_component(id, &schema)
            .context("Failed to register component tools")?;

        let arc_component = Arc::new(component);
        self.components
            .write()
            .await
            .insert(id.to_string(), arc_component);

        info!("Loaded component '{}' from path '{}'", id, path);
        Ok(())
    }

    /// Unloads the component with the specified id.
    #[instrument(skip(self))]
    pub async fn unload_component(&self, id: &str) -> Result<()> {
        debug!("Unloading component");
        let mut comps = self.components.write().await;
        comps.remove(id);

        let mut tx = self.db.begin().await?;

        let result = sqlx::query("DELETE FROM components WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .context("Failed to remove component from database")?;

        tx.commit().await?;

        self.registry.write().await.unregister_component(id);

        if result.rows_affected() > 0 {
            info!("Unloaded component '{}'", id);
            Ok(())
        } else {
            warn!("Component '{}' not found", id);
            bail!("Component with id '{}' not found", id);
        }
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

    #[instrument(skip(self))]
    async fn load_from_db(&self, id: &str) -> Result<Arc<Component>> {
        debug!("Loading component from database");
        let record = sqlx::query("SELECT path FROM components WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.db)
            .await
            .context("Failed to query component from database")?;

        if let Some(record) = record {
            let path: String = record.try_get("path")?;
            debug!("Found component path in database: {}", path);
            let component = Component::from_file(&self.engine, &path)
                .with_context(|| format!("Failed to load component from path: {}", path))?;
            let arc_component = Arc::new(component);
            self.components
                .write()
                .await
                .insert(id.to_string(), arc_component.clone());
            info!("Loaded component '{}' from database", id);
            Ok(arc_component)
        } else {
            warn!("Component '{}' not found in database", id);
            bail!("Component with id '{}' not found", id)
        }
    }

    /// Returns the requested component. If no id is provided and only one component is loaded,
    /// that component is returned; otherwise an error is raised.
    #[instrument(skip(self))]
    pub async fn get_component(&self, component_id: Option<&str>) -> Result<Arc<Component>> {
        match component_id {
            Some(id) => {
                if let Some(comp) = self.components.read().await.get(id).cloned() {
                    info!("Retrieved component '{}' from memory", id);
                    return Ok(comp);
                }
                debug!("Component not found in memory, trying database");
                self.load_from_db(id).await
            }
            None => {
                let comps = self.components.read().await;
                if comps.len() == 1 {
                    info!("Retrieved single component from memory");
                    return Ok(comps.values().next().unwrap().clone());
                }
                drop(comps);

                let count: i64 = sqlx::query("SELECT COUNT(*) as count FROM components")
                    .fetch_one(&self.db)
                    .await
                    .context("Failed to count components in database")?
                    .try_get("count")?;

                if count == 1 {
                    let record = sqlx::query("SELECT id FROM components LIMIT 1")
                        .fetch_one(&self.db)
                        .await
                        .context("Failed to get single component from database")?;
                    let id: String = record.try_get("id")?;
                    debug!("Found single component in database with id: {}", id);
                    self.load_from_db(&id).await
                } else {
                    warn!("Multiple components found, but no specific id provided");
                    bail!("Multiple components loaded. Please specify component id.")
                }
            }
        }
    }

    #[instrument(skip(self))]
    pub async fn list_components(&self) -> Result<Vec<String>> {
        debug!("Listing all components");
        let memory_components: HashSet<String> =
            self.components.read().await.keys().cloned().collect();

        let db_components: HashSet<String> = sqlx::query("SELECT id FROM components")
            .fetch_all(&self.db)
            .await
            .context("Failed to query components from database")?
            .into_iter()
            .map(|row| row.try_get("id"))
            .collect::<Result<_, _>>()?;

        let mut all_components: Vec<String> =
            memory_components.union(&db_components).cloned().collect();
        all_components.sort();
        info!("Found {} components in total", all_components.len());
        Ok(all_components)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    use serde_json::json;
    use tempfile::TempDir;
    use test_log::test;

    use super::*;

    async fn create_test_manager() -> Result<LifecycleManager> {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.async_support(true);
        let engine = Arc::new(wasmtime::Engine::new(&config)?);
        LifecycleManager::new_with_db_url(engine, "sqlite::memory:").await
    }

    async fn build_example_component() -> Result<PathBuf> {
        let cwd = std::env::current_dir()?;
        println!("CWD: {}", cwd.display());
        let component_path =
            cwd.join("../../examples/fetch-rs/target/wasm32-wasip1/release/fetch_rs.wasm");

        if !component_path.exists() {
            let status = Command::new("cargo")
                .current_dir(cwd.join("../../examples/fetch-rs"))
                .args(["component", "build", "--release"])
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

        let load_result = manager
            .load_component("test-id", component_path.to_str().unwrap())
            .await;
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

        let load_result = manager
            .load_component("test-id", "/path/to/nonexistent")
            .await;
        assert!(load_result.is_err());

        let unload_result = manager.unload_component("non-existent").await;
        assert!(unload_result.is_err());

        Ok(())
    }

    #[test(tokio::test)]
    async fn test_list_components() -> Result<()> {
        let manager = create_test_manager().await?;
        let components = manager.list_components().await?;
        assert!(components.is_empty());

        sqlx::query("INSERT INTO components (id, path) VALUES (?, ?)")
            .bind("test-component")
            .bind("/path/to/component")
            .execute(&manager.db)
            .await?;

        let components = manager.list_components().await?;
        assert_eq!(components.len(), 1);
        assert_eq!(components[0], "test-component");

        Ok(())
    }

    #[test(tokio::test)]
    async fn test_get_component() -> Result<()> {
        let manager = create_test_manager().await?;
        assert!(manager.get_component(Some("non-existent")).await.is_err());
        assert!(manager.get_component(None).await.is_err());

        sqlx::query("INSERT INTO components (id, path) VALUES (?, ?)")
            .bind("test-component")
            .bind("/path/to/component")
            .execute(&manager.db)
            .await?;

        assert!(manager.get_component(Some("test-component")).await.is_err());
        Ok(())
    }

    #[test(tokio::test)]
    async fn test_concurrent_access() -> Result<()> {
        let manager = Arc::new(create_test_manager().await?);
        let component_id = "test-component";
        let component_path = "/path/to/component";

        let manager_clone = manager.clone();
        let write_handle = tokio::spawn(async move {
            sqlx::query("INSERT INTO components (id, path) VALUES (?, ?)")
                .bind(component_id)
                .bind(component_path)
                .execute(&manager_clone.db)
                .await
        });

        write_handle.await??;

        let components = manager.list_components().await?;
        assert_eq!(components.len(), 1);
        assert_eq!(components[0], component_id);

        Ok(())
    }

    #[test(tokio::test)]
    async fn test_duplicate_component_id() -> Result<()> {
        let manager = create_test_manager().await?;
        let component_id = "test-component";

        sqlx::query("INSERT OR REPLACE INTO components (id, path) VALUES (?, ?)")
            .bind(component_id)
            .bind("/path/1")
            .execute(&manager.db)
            .await?;

        sqlx::query("INSERT OR REPLACE INTO components (id, path) VALUES (?, ?)")
            .bind(component_id)
            .bind("/path/2")
            .execute(&manager.db)
            .await?;

        let components = manager.list_components().await?;
        assert_eq!(components.len(), 1);
        assert_eq!(components[0], component_id);

        Ok(())
    }

    #[test(tokio::test)]
    async fn test_memory_db_sync() -> Result<()> {
        let manager = create_test_manager().await?;
        let component_id = "test-component";
        let component_path = "/path/to/component";

        sqlx::query("INSERT INTO components (id, path) VALUES (?, ?)")
            .bind(component_id)
            .bind(component_path)
            .execute(&manager.db)
            .await?;

        let components_db = manager.list_components().await?;
        assert_eq!(components_db.len(), 1);

        let components_memory = manager
            .components
            .read()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        assert!(components_memory.is_empty());

        Ok(())
    }

    #[test(tokio::test)]
    async fn test_tool_registry_persistence() -> Result<()> {
        let db_file = tempfile::NamedTempFile::new()?;
        let db_url = format!("sqlite:{}", db_file.path().to_str().unwrap());

        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.async_support(true);
        let engine = Arc::new(wasmtime::Engine::new(&config)?);

        let manager1 = LifecycleManager::new_with_db_url(engine.clone(), &db_url).await?;

        let component_path = build_example_component().await?;

        manager1
            .load_component("comp1", component_path.to_str().unwrap())
            .await?;

        let manager2 = LifecycleManager::new_with_db_url(engine, &db_url).await?;

        let component_id = manager2.get_component_id_for_tool("fetch").await?;
        assert_eq!(component_id, "comp1");

        Ok(())
    }

    #[test(tokio::test)]
    async fn test_component_reload() -> Result<()> {
        let manager = create_test_manager().await?;
        let component_path = build_example_component().await?;

        manager
            .load_component("comp1", component_path.to_str().unwrap())
            .await?;

        let component_id = manager.get_component_id_for_tool("fetch").await?;
        assert_eq!(component_id, "comp1");

        manager
            .load_component("comp1", component_path.to_str().unwrap())
            .await?;

        let component_id = manager.get_component_id_for_tool("fetch").await?;
        assert_eq!(component_id, "comp1");

        Ok(())
    }

    #[test(tokio::test)]
    async fn test_component_path_update() -> Result<()> {
        let manager = create_test_manager().await?;
        let component_path = build_example_component().await?;

        manager
            .load_component("comp1", component_path.to_str().unwrap())
            .await?;

        manager
            .load_component("comp1", component_path.to_str().unwrap())
            .await?;

        let component_id = manager.get_component_id_for_tool("fetch").await?;
        assert_eq!(component_id, "comp1");

        Ok(())
    }

    #[test(tokio::test)]
    async fn test_tool_name_collisions() -> Result<()> {
        let manager = create_test_manager().await?;
        let component_path = build_example_component().await?;

        manager
            .load_component("comp1", component_path.to_str().unwrap())
            .await?;
        manager
            .load_component("comp2", component_path.to_str().unwrap())
            .await?;

        let result = manager.get_component_id_for_tool("fetch").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Multiple components found"));

        Ok(())
    }
}
