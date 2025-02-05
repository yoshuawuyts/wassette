use anyhow::Result;
use mcp_sdk::types::{ListRequest, ResourcesListResponse};

pub fn handle_resources_list(_req: ListRequest) -> Result<ResourcesListResponse> {
    Ok(ResourcesListResponse {
        resources: vec![],
        next_cursor: None,
        meta: None,
    })
}
