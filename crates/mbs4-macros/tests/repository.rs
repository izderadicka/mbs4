#![allow(unexpected_cfgs)]
use garde::Validate;
use mbs4_macros::Repository;
use serde::{Deserialize, Serialize};

//these are required for macro to work

pub use mbs4_dal::{Batch, ChosenDB, ChosenRow, Error, FromRowPrefixed, ListingParams, MAX_LIMIT};
pub mod error {
    pub use mbs4_dal::error::{Error, Result};
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow, Repository)]
pub struct Language {
    #[spec(id)]
    id: i64,
    #[garde(length(min = 1, max = 255))]
    name: String,
    #[garde(length(min = 2, max = 4))]
    code: String,
    #[garde(range(min = 0))]
    #[spec(version)]
    version: i64,
}

#[test]
fn test_repository() {
    let language = CreateLanguage {
        name: "English".to_string(),
        code: "en".to_string(),
    };

    assert!(language.validate().is_ok());

    let _language_full = Language {
        id: 1,
        name: "English".to_string(),
        code: "en".to_string(),
        version: 1,
    };

    let _language_short = LanguageShort {
        id: 1,
        name: "English".to_string(),
        code: "en".to_string(),
    };

    assert_eq!(3, VALID_ORDER_FIELDS.len());
}
