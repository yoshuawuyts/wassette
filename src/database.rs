use std::path::PathBuf;
use std::{env, fs};

use anyhow::Result;

/// Get the default installation path for the database based on the OS
pub fn get_default_db_path() -> PathBuf {
    if cfg!(target_os = "windows") {
        let local_app_data = env::var("LOCALAPPDATA")
            .unwrap_or_else(|_| env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".to_string()));
        PathBuf::from(local_app_data)
            .join("mcp-wasmtime")
            .join("components.db")
    } else if cfg!(target_os = "macos") {
        let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("mcp-wasmtime")
            .join("components.db")
    } else {
        let xdg_data_home = env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
            let home = env::var("HOME").unwrap_or_else(|_| "/".to_string());
            format!("{}/.local/share", home)
        });
        PathBuf::from(xdg_data_home)
            .join("mcp-wasmtime")
            .join("components.db")
    }
}

/// Resolve the database URL based on the following priority:
/// 1. DATABASE_URL environment variable
/// 2. Existing database in default installation path
/// 3. Create a new database in default installation path
pub async fn resolve_database_url() -> Result<String> {
    if let Ok(url) = env::var("DATABASE_URL") {
        return Ok(url);
    }

    let default_path = get_default_db_path();

    if default_path.exists() {
        return Ok(format!("sqlite:{}", default_path.display()));
    }

    if let Some(parent) = default_path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&default_path, "")?;

    Ok(format!("sqlite:{}", default_path.display()))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn test_database_url_resolution() -> Result<()> {
        env::set_var("DATABASE_URL", "sqlite:test.db");
        assert_eq!(resolve_database_url().await?, "sqlite:test.db");
        env::remove_var("DATABASE_URL");

        let temp_dir = tempdir()?;

        let original_home = env::var("HOME");
        env::set_var("HOME", temp_dir.path());

        let expected_path = if cfg!(target_os = "macos") {
            temp_dir
                .path()
                .join("Library")
                .join("Application Support")
                .join("mcp-wasmtime")
                .join("components.db")
        } else if cfg!(target_os = "windows") {
            if let Ok(home) = original_home {
                env::set_var("HOME", home);
            }
            return Ok(());
        } else {
            temp_dir
                .path()
                .join(".local")
                .join("share")
                .join("mcp-wasmtime")
                .join("components.db")
        };

        let db_url = resolve_database_url().await?;
        assert!(db_url.contains("components.db"));
        assert!(
            expected_path.exists(),
            "Database file was not created at the expected path: {:?}",
            expected_path
        );

        if let Ok(home) = original_home {
            env::set_var("HOME", home);
        } else {
            env::remove_var("HOME");
        }

        Ok(())
    }
}
