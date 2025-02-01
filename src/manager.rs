use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::RwLock;
use tracing;

use wasmtime::component::Component;
use wasmtime::Engine;

/// A manager that handles the dynamic lifecycle of WebAssembly components.
#[derive(Clone)]
pub struct LifecycleManager {
    pub engine: Arc<Engine>,
    pub(crate) components: Arc<RwLock<HashMap<String, Arc<Component>>>>,
    tools_changed_sender: UnboundedSender<()>,
}

impl LifecycleManager {
    pub fn new(engine: Arc<Engine>, tools_changed_sender: UnboundedSender<()>) -> Self {
        Self {
            engine,
            components: Arc::new(RwLock::new(HashMap::new())),
            tools_changed_sender,
        }
    }

    /// Loads a new component from a local path (or, in the future, from an OCI URL)
    /// and associates it with the given id.
    pub async fn load_component(&self, id: &str, path: &str) -> Result<()> {
        let component = Component::from_file(&self.engine, path)
            .with_context(|| format!("Failed to load component from path: {}", path))?;
        let arc_component = Arc::new(component);
        self.components
            .write()
            .await
            .insert(id.to_string(), arc_component);
        tracing::info!("Loaded component '{}' from path '{}'", id, path);
        self.tools_changed_sender
            .send(())
            .expect("Failed to send tools changed message");
        Ok(())
    }

    /// Unloads the component with the specified id.
    pub async fn unload_component(&self, id: &str) -> Result<()> {
        let mut comps = self.components.write().await;
        if comps.remove(id).is_some() {
            tracing::info!("Unloaded component '{}'", id);
            self.tools_changed_sender
                .send(())
                .expect("Failed to send tools changed message");
            Ok(())
        } else {
            bail!("Component with id '{}' not found", id);
        }
    }

    /// Returns the requested component. If no id is provided and only one component is loaded,
    /// that component is returned; otherwise an error is raised.
    pub async fn get_component(&self, component_id: Option<&str>) -> Result<Arc<Component>> {
        let comps = self.components.read().await;
        if let Some(id) = component_id {
            if let Some(comp) = comps.get(id) {
                return Ok(comp.clone());
            } else {
                bail!("Component with id '{}' not found", id);
            }
        } else {
            if comps.len() == 1 {
                return Ok(comps.values().next().unwrap().clone());
            } else {
                bail!("Multiple components loaded. Please specify component id.");
            }
        }
    }
}
