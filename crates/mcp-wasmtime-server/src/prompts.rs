use anyhow::Result;
use mcp_sdk::types::{ListRequest, PromptsListResponse};

pub fn handle_prompts_list(_req: ListRequest) -> Result<PromptsListResponse> {
    Ok(PromptsListResponse {
        prompts: vec![],
        next_cursor: None,
        meta: None,
    })
}
