use std::sync::Arc;

use lifecycle_proto::lifecycle::lifecycle_manager_service_client::LifecycleManagerServiceClient;

mod components;
mod prompts;
mod resources;
mod tools;

pub use prompts::handle_prompts_list;
pub use resources::handle_resources_list;
use tonic::transport::Channel;
pub use tools::{handle_tools_call, handle_tools_list};

pub type GrpcClient = Arc<tokio::sync::Mutex<LifecycleManagerServiceClient<Channel>>>;
