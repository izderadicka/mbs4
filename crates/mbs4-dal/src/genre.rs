use garde::Validate;
use mbs4_macros::ValueRepository as Repository;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Validate, Repository)]
pub struct CreateGenre {
    #[garde(length(min = 1, max = 255))]
    name: String,
    #[garde(range(min = 0))]
    version: Option<i64>,
}
