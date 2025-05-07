use std::collections::HashMap;
use std::path::Path;
use std::{env, fs};

use anyhow::{Result, anyhow};
use regex::Regex;

#[derive(Debug, serde::Deserialize)]
struct PolicyFile {
    env: Option<HashMap<String, String>>,
}

fn process_env_vars(map: &mut HashMap<String, String>) {
    // FYI the actual format after TOML parsing: { ENV_VAR }
    let pattern = r"\{\s*([A-Za-z0-9_]+)\s*\}";
    let re = Regex::new(pattern).unwrap();

    for (_, value) in map.iter_mut() {
        if re.is_match(value) {
            if let Some(caps) = re.captures(value) {
                if let Some(env_var_name) = caps.get(1) {
                    let env_var_name = env_var_name.as_str();
                    if let Ok(env_val) = env::var(env_var_name) {
                        *value = env_val;
                    }
                }
            }
        }
    }
}

pub fn load_policy<P: AsRef<Path>>(path: P) -> Result<HashMap<String, String>> {
    let content = fs::read_to_string(path)?;
    let policy: PolicyFile = toml::from_str(&content)?;
    let mut env_vars = policy
        .env
        .ok_or(anyhow!("Missing [env] section in policy file"))?;

    process_env_vars(&mut env_vars);

    Ok(env_vars)
}

#[cfg(test)]
mod tests {
    use std::env;
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
    fn test_env_var_substitution() {
        let key = "TEST_ENV_VAR";
        let val = "test_value";
        let old_value = env::var_os(key);
        unsafe {
            env::set_var(key, val);
        }

        let mut file = NamedTempFile::new().unwrap();

        writeln!(file, "[env]").unwrap();
        writeln!(file, "FOO = 'bar'").unwrap();
        writeln!(file, "ENV_TEST = '{{ TEST_ENV_VAR }}'").unwrap();

        let vars = load_policy(file.path()).unwrap();
        assert_eq!(vars.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(vars.get("ENV_TEST"), Some(&val.to_string()));

        match old_value {
            Some(value) => unsafe { env::set_var(key, value) },
            None => unsafe { env::remove_var(key) },
        }
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
