#[allow(warnings)]
mod bindings;

use std::{env, fs, path::{Path, PathBuf}};

use anyhow::{Result, anyhow};

use bindings::{exports::mossaka::mcp::tool_server::{CallToolRequest, CallToolResponse, Guest, ListToolsRequest, ListToolsResponse}, mossaka::mcp::types::{ToolDefinition, ToolResponseContent, ToolResponseText}};
use serde_json::Value;

struct Component;

impl Guest for Component {
    fn call_tool(req: CallToolRequest) -> CallToolResponse {
        // Extract the tool name
        let name = req.name;
        // Extract the arguments as string, default to empty "{}" if none
        let args_str = req.arguments.unwrap_or_else(|| "{}".to_string());
    
        // Parse arguments as JSON so we can extract fields like "path" or "pattern".
        let args_json: Value = match serde_json::from_str(&args_str) {
            Ok(json) => json,
            Err(e) => {
                return CallToolResponse {
                    content: vec![ToolResponseContent::Text(ToolResponseText {
                        text: format!("Error parsing arguments JSON: {}", e),
                    })],
                    is_error: Some(true),
                    meta: None,
                }
            }
        };
    
        let content = match name.as_str() {
            "read_file" => {
                // Read a file
                match get_path(&args_json) {
                    Ok(path) => match fs::read_to_string(&path) {
                        Ok(text) => ToolResponseContent::Text(ToolResponseText { text }),
                        Err(e) => {
                            return CallToolResponse {
                                content: vec![ToolResponseContent::Text(ToolResponseText {
                                    text: format!("Failed to read file: {}", e),
                                })],
                                is_error: Some(true),
                                meta: None,
                            }
                        }
                    },
                    Err(e) => {
                        return CallToolResponse {
                            content: vec![ToolResponseContent::Text(ToolResponseText { text: e.to_string() })],
                            is_error: Some(true),
                            meta: None,
                        }
                    }
                }
            }
    
            "list_directory" => {
                // List entries in a directory
                match get_path(&args_json) {
                    Ok(path) => {
                        let mut text = String::new();
                        let entries = match fs::read_dir(&path) {
                            Ok(e) => e,
                            Err(e) => {
                                return CallToolResponse {
                                    content: vec![ToolResponseContent::Text(ToolResponseText {
                                        text: format!("Failed to read directory: {}", e),
                                    })],
                                    is_error: Some(true),
                                    meta: None,
                                }
                            }
                        };
                        for entry_result in entries {
                            match entry_result {
                                Ok(entry) => {
                                    let prefix = match entry.file_type() {
                                        Ok(ft) if ft.is_dir() => "[DIR]",
                                        Ok(_) => "[FILE]",
                                        Err(_) => "[UNKNOWN]",
                                    };
                                    let file_name = entry.file_name();
                                    text.push_str(&format!(
                                        "{prefix} {}\n",
                                        file_name.to_string_lossy()
                                    ));
                                }
                                Err(e) => {
                                    text.push_str(&format!("Error reading entry: {e}\n"));
                                }
                            }
                        }
                        ToolResponseContent::Text(ToolResponseText { text })
                    }
                    Err(e) => {
                        return CallToolResponse {
                            content: vec![ToolResponseContent::Text(ToolResponseText { text: e.to_string() })],
                            is_error: Some(true),
                            meta: None,
                        }
                    }
                }
            }
    
            "search_files" => {
                // Recursively search for files matching a pattern
                let path = match get_path(&args_json) {
                    Ok(p) => p,
                    Err(e) => {
                        return CallToolResponse {
                            content: vec![ToolResponseContent::Text(ToolResponseText { text: e.to_string() })],
                            is_error: Some(true),
                            meta: None,
                        }
                    }
                };
                let pattern = match args_json.get("pattern") {
                    Some(v) => match v.as_str() {
                        Some(s) => s,
                        None => {
                            return CallToolResponse {
                                content: vec![ToolResponseContent::Text(ToolResponseText {
                                    text: "'pattern' must be a string".to_string(),
                                })],
                                is_error: Some(true),
                                meta: None,
                            }
                        }
                    },
                    None => {
                        return CallToolResponse {
                            content: vec![ToolResponseContent::Text(ToolResponseText {
                                text: "Missing 'pattern' field".to_string(),
                            })],
                            is_error: Some(true),
                            meta: None,
                        }
                    }
                };
                let mut matches = Vec::new();
                if let Err(e) = search_directory(&path, pattern, &mut matches) {
                    return CallToolResponse {
                        content: vec![ToolResponseContent::Text(ToolResponseText {
                            text: format!("Search error: {}", e),
                        })],
                        is_error: Some(true),
                        meta: None,
                    };
                }
                ToolResponseContent::Text(ToolResponseText {
                    text: matches.join("\n"),
                })
            }
    
            "get_file_info" => {
                // Retrieve metadata about a file or directory
                match get_path(&args_json) {
                    Ok(path) => match fs::metadata(&path) {
                        Ok(metadata) => {
                            ToolResponseContent::Text(ToolResponseText { text: format!("{:?}", metadata) })
                        }
                        Err(e) => {
                            return CallToolResponse {
                                content: vec![ToolResponseContent::Text(ToolResponseText {
                                    text: format!("Failed to get metadata: {}", e),
                                })],
                                is_error: Some(true),
                                meta: None,
                            }
                        }
                    },
                    Err(e) => {
                        return CallToolResponse {
                            content: vec![ToolResponseContent::Text(ToolResponseText { text: e.to_string() })],
                            is_error: Some(true),
                            meta: None,
                        }
                    }
                }
            }
    
            "list_allowed_directories" => {
                // Return an empty list (JSON array) as a text
                ToolResponseContent::Text(ToolResponseText {
                    text: "[]".to_string(),
                })
            }
    
            // Anything else is unknown
            _ => {
                return CallToolResponse {
                    content: vec![ToolResponseContent::Text(ToolResponseText {
                        text: format!("Unknown tool: {}", name),
                    })],
                    is_error: Some(true),
                    meta: None,
                };
            }
        };
    
        // If we got here, everything succeeded
        CallToolResponse {
            content: vec![content],
            is_error: None,
            meta: None,
        }
    }

    fn list_tools(_req: ListToolsRequest) -> ListToolsResponse {
        ListToolsResponse {
            tools: vec![
                ToolDefinition {
                    name: "read_file".to_string(),
                    description: Some(
                        "Read the complete contents of a file from the file system. \
                         Handles various text encodings and provides detailed error messages \
                         if the file cannot be read. Use this tool when you need to examine \
                         the contents of a single file. Only works within allowed directories."
                            .to_string(),
                    ),
                    input_schema: r#"{
      "type": "object",
      "properties": {
        "path": {
          "type": "string"
        }
      },
      "required": ["path"]
    }"#
                    .to_string(),
                },
                ToolDefinition {
                    name: "list_directory".to_string(),
                    description: Some(
                        "Get a detailed listing of all files and directories in a specified path. \
                         Results clearly distinguish between files and directories with [FILE] and [DIR] \
                         prefixes. This tool is essential for understanding directory structure and \
                         finding specific files within a directory. Only works within allowed directories."
                            .to_string(),
                    ),
                    input_schema: r#"{
      "type": "object",
      "properties": {
        "path": {
          "type": "string"
        }
      },
      "required": ["path"]
    }"#
                    .to_string(),
                },
                ToolDefinition {
                    name: "search_files".to_string(),
                    description: Some(
                        "Recursively search for files and directories matching a pattern. \
                         Searches through all subdirectories from the starting path. The search \
                         is case-insensitive and matches partial names. Returns full paths to all \
                         matching items. Great for finding files when you don't know their exact location. \
                         Only searches within allowed directories."
                            .to_string(),
                    ),
                    input_schema: r#"{
      "type": "object",
      "properties": {
        "path": {
          "type": "string"
        },
        "pattern": {
          "type": "string"
        }
      },
      "required": ["path", "pattern"]
    }"#
                    .to_string(),
                },
                ToolDefinition {
                    name: "get_file_info".to_string(),
                    description: Some(
                        "Retrieve detailed metadata about a file or directory. Returns comprehensive \
                         information including size, creation time, last modified time, permissions, \
                         and type. This tool is perfect for understanding file characteristics \
                         without reading the actual content. Only works within allowed directories."
                            .to_string(),
                    ),
                    input_schema: r#"{
      "type": "object",
      "properties": {
        "path": {
          "type": "string"
        }
      },
      "required": ["path"]
    }"#
                    .to_string(),
                },
            ],
            next_cursor: None,
            meta: None,
        }
    }
}


fn search_directory(dir: &Path, pattern: &str, matches: &mut Vec<String>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();

        // Check if the current file/directory matches the pattern
        if name.contains(&pattern.to_lowercase()) {
            matches.push(path.to_string_lossy().to_string());
        }

        // Recursively search subdirectories
        if path.is_dir() {
            search_directory(&path, pattern, matches)?;
        }
    }
    Ok(())
}

fn get_path(args: &Value) -> Result<PathBuf> {
    let path_str = args["path"]
        .as_str()
        .ok_or_else(|| anyhow!("Missing 'path' field"))?;

    if path_str == "~" || path_str.starts_with("~/") {
        let home_dir = env::var("HOME")
            .map_err(|_| anyhow!("Cannot determine home directory from $HOME"))?;

        if path_str == "~" {
            return Ok(PathBuf::from(home_dir));
        }
        let suffix = &path_str[2..];
        let combined = Path::new(&home_dir).join(suffix);
        return Ok(combined);
    }

    // If it doesn't start with "~", return as-is
    Ok(PathBuf::from(path_str))
}

bindings::export!(Component with_types_in bindings);
