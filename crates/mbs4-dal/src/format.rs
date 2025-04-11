use garde::Validate;
use mbs4_macros::ValueRepository as Repository;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Validate, Repository)]
pub struct CreateFormat {
    #[garde(length(min = 1, max = 255))]
    name: String,
    #[garde(length(min = 3, max = 255))]
    mime_type: String,
    #[garde(length(min = 1, max = 32))]
    extension: String,
    #[garde(range(min = 0))]
    version: Option<i64>,
}
