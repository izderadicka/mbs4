use garde::Validate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Validate)]
pub struct CreateAuthor {
    #[garde(length(min = 1, max = 255))]
    last_name: String,
    #[garde(length(min = 1, max = 255))]
    first_name: Option<String>,
    #[garde(length(min = 1, max = 5000))]
    description: Option<String>,
    #[garde(range(min = 0))]
    version: Option<i64>,
}
