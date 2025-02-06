use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use sqlx::{Pool, Row, Sqlite, SqlitePool};
use tokio::sync::RwLock;
use tracing::{debug, info, instrument, warn};
use wasmtime::component::Component;
use wasmtime::Engine;

/// A manager that handles the dynamic lifecycle of WebAssembly components.
pub struct LifecycleManager {
    pub engine: Arc<Engine>,
    pub components: Arc<RwLock<HashMap<String, Arc<Component>>>>,
    pub db: Pool<Sqlite>,
}

impl LifecycleManager {
    #[instrument(skip(engine))]
    pub async fn new(engine: Arc<Engine>) -> Result<Self> {
        Self::new_with_db_url(engine, "sqlite:components.db").await
    }

    #[instrument(skip(engine))]
    pub async fn new_with_db_url(engine: Arc<Engine>, db_url: &str) -> Result<Self> {
        debug!("Creating new LifecycleManager");
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

        info!("LifecycleManager initialized successfully");
        Ok(Self {
            engine,
            components: Arc::new(RwLock::new(HashMap::new())),
            db,
        })
    }

    /// Loads a new component from a local path (or, in the future, from an OCI URL)
    /// and associates it with the given id.
    #[instrument(skip(self))]
    pub async fn load_component(&self, id: &str, path: &str) -> Result<()> {
        debug!("Loading component from path");
        let component = Component::from_file(&self.engine, path)
            .with_context(|| format!("Failed to load component from path: {}", path))?;
        let arc_component = Arc::new(component);
        self.components
            .write()
            .await
            .insert(id.to_string(), arc_component);

        sqlx::query("INSERT OR REPLACE INTO components (id, path) VALUES (?, ?)")
            .bind(id)
            .bind(path)
            .execute(&self.db)
            .await
            .context("Failed to store component in database")?;

        info!("Loaded component '{}' from path '{}'", id, path);
        Ok(())
    }

    /// Unloads the component with the specified id.
    #[instrument(skip(self))]
    pub async fn unload_component(&self, id: &str) -> Result<()> {
        debug!("Unloading component");
        let mut comps = self.components.write().await;
        comps.remove(id);

        let result = sqlx::query("DELETE FROM components WHERE id = ?")
            .bind(id)
            .execute(&self.db)
            .await
            .context("Failed to remove component from database")?;

        if result.rows_affected() > 0 {
            info!("Unloaded component '{}'", id);
            Ok(())
        } else {
            warn!("Component '{}' not found", id);
            bail!("Component with id '{}' not found", id);
        }
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
    use tempfile::NamedTempFile;
    use test_log::test;

    use super::*;

    async fn create_test_manager() -> Result<LifecycleManager> {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.async_support(true);
        let engine = Arc::new(wasmtime::Engine::new(&config)?);
        LifecycleManager::new_with_db_url(engine, "sqlite::memory:").await
    }

    async fn create_test_manager_with_db_path(db_path: &str) -> Result<LifecycleManager> {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        config.async_support(true);
        let engine = Arc::new(wasmtime::Engine::new(&config)?);
        LifecycleManager::new_with_db_url(engine, &format!("sqlite:{}", db_path)).await
    }

    #[test(tokio::test)]
    async fn test_new_manager() -> Result<()> {
        let _manager = create_test_manager().await?;
        Ok(())
    }

    #[test(tokio::test)]
    async fn test_load_and_unload_component() -> Result<()> {
        let manager = create_test_manager().await?;
        let temp_file = NamedTempFile::new()?;
        let path = temp_file.path().to_str().unwrap();

        let load_result = manager.load_component("test-id", path).await;
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
        Ok(())
    }

    #[test(tokio::test)]
    async fn test_get_component() -> Result<()> {
        let manager = create_test_manager().await?;
        assert!(manager.get_component(Some("non-existent")).await.is_err());
        assert!(manager.get_component(None).await.is_err());
        Ok(())
    }

    #[test(tokio::test)]
    async fn test_persistence_across_restarts() -> Result<()> {
        let temp_file = NamedTempFile::new()?;
        let db_path = temp_file.path().to_str().unwrap();

        let component_id = "test-component";
        let component_path = "/path/to/component";

        {
            let manager = create_test_manager_with_db_path(db_path).await?;
            sqlx::query("INSERT INTO components (id, path) VALUES (?, ?)")
                .bind(component_id)
                .bind(component_path)
                .execute(&manager.db)
                .await?;
        }

        let manager = create_test_manager_with_db_path(db_path).await?;
        let components = manager.list_components().await?;
        assert_eq!(components.len(), 1);
        assert_eq!(components[0], component_id);

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
    async fn test_component_cleanup() -> Result<()> {
        let manager = create_test_manager().await?;
        let component_id = "test-component";
        let component_path = "/path/to/component";

        sqlx::query("INSERT OR REPLACE INTO components (id, path) VALUES (?, ?)")
            .bind(component_id)
            .bind(component_path)
            .execute(&manager.db)
            .await?;

        let components = manager.list_components().await?;
        assert_eq!(components.len(), 1);

        let db_count: i64 = sqlx::query("SELECT COUNT(*) as count FROM components")
            .fetch_one(&manager.db)
            .await?
            .try_get("count")?;
        assert_eq!(db_count, 1);

        manager.unload_component(component_id).await?;

        let components = manager.list_components().await?;
        assert!(components.is_empty());

        let db_count: i64 = sqlx::query("SELECT COUNT(*) as count FROM components")
            .fetch_one(&manager.db)
            .await?
            .try_get("count")?;
        assert_eq!(db_count, 0);

        Ok(())
    }

    #[test(tokio::test)]
    async fn test_multiple_components() -> Result<()> {
        let manager = create_test_manager().await?;

        for i in 0..5 {
            sqlx::query("INSERT OR REPLACE INTO components (id, path) VALUES (?, ?)")
                .bind(format!("component-{}", i))
                .bind(format!("/path/{}", i))
                .execute(&manager.db)
                .await?;
        }

        let components = manager.list_components().await?;
        assert_eq!(components.len(), 5);
        assert!(components.contains(&"component-0".to_string()));
        assert!(components.contains(&"component-4".to_string()));

        Ok(())
    }
}
