use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Result, anyhow};

#[derive(Debug, serde::Deserialize)]
struct PolicyFile {
    env: Option<HashMap<String, String>>,
}

pub fn load_policy<P: AsRef<Path>>(path: P) -> Result<HashMap<String, String>> {
    let content = fs::read_to_string(path)?;
    let policy: PolicyFile = toml::from_str(&content)?;
    policy
        .env
        .ok_or(anyhow!("Missing [env] section in policy file"))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::load_policy;

    #[test]
    fn test_valid_policy() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "[env]\nFOO = 'bar'\nBAZ = 'qux'").unwrap();
        let vars = load_policy(file.path()).unwrap();
        assert_eq!(vars.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(vars.get("BAZ"), Some(&"qux".to_string()));
    }

    #[test]
    #[should_panic]
    fn test_missing_env_section() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "[not_env]\nFOO = 'bar'").unwrap();
        load_policy(file.path()).unwrap();
    }

    #[test]
    #[should_panic]
    fn test_malformed_policy() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "[env\nFOO = 'bar'").unwrap();
        load_policy(file.path()).unwrap();
    }
}
