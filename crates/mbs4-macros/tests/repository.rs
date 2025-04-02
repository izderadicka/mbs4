use garde::Validate;
use mbs4_macros::Repository;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, Validate, Repository)]
pub struct CreateLanguage {
    #[garde(length(min = 1, max = 255))]
    name: String,
    #[garde(length(min = 2, max = 4))]
    code: String,
    #[garde(range(min = 0))]
    version: Option<i64>,
}

#[test]
fn test_repository() {
    let language = CreateLanguage {
        name: "English".to_string(),
        code: "en".to_string(),
        version: None,
    };
    assert!(language.validate().is_ok());

    let language_full = Language {
        id: 1,
        name: "English".to_string(),
        code: "en".to_string(),
        version: 1,
    };
}
