use spin_sdk::http::{send, RequestBuilder, Request, Response};

#[allow(warnings)]
mod bindings;

use bindings::Guest;
use serde_json::Value;

struct Component;

impl Guest for Component {
    fn fetch(url: String) -> Result<String, String> {
        spin_executor::run(async move {
            let request = Request::get(url);
            let response: Response = send(request).await.map_err(|e| e.to_string())?;
            println!("Response: {:?}", response);
            let status = response.status();
            if !(200..300).contains(status) {
                return Err(format!("Request failed with status code: {}", status));
            }
            println!("Response status: {}", status);
            let body_str = String::from_utf8_lossy(response.body());
            println!("Response body: {}", body_str);
            let json: Value = serde_json::from_str(&body_str).map_err(|e| e.to_string())?;
            let markdown_body = json_to_markdown(&json);
            let markdown = format!(
                "# Response\n\n## Status Code\n\n`{}`\n\n## Body\n\n{}",
                status,
                markdown_body
            );

            Ok(markdown)
        })
    }
}

fn json_to_markdown(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut markdown = String::new();
            for (key, val) in map {
                markdown.push_str(&format!("### {}\n\n{}\n\n", key, json_to_markdown(val)));
            }
            markdown
        }
        Value::Array(arr) => {
            let mut markdown = String::new();
            for (i, val) in arr.iter().enumerate() {
                markdown.push_str(&format!("1. {}\n", json_to_markdown(val)));
                if i < arr.len() - 1 {
                    markdown.push('\n');
                }
            }
            markdown
        }
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
    }
}
bindings::export!(Component with_types_in bindings);
