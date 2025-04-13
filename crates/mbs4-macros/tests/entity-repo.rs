use std::collections::HashSet;

use mbs4_macros::Repository;
use serde::Serialize;

//these are required for macro to work

pub use mbs4_dal::{ChosenDB, ListingParams, MAX_LIMIT};
pub mod error {
    pub use mbs4_dal::error::{Error, Result};
}

#[derive(Debug, Serialize, Clone, sqlx::FromRow, Repository)]
pub struct Author {
    #[spec(id)]
    pub id: i64,
    #[garde(length(min = 1, max = 255))]
    pub last_name: String,
    #[garde(length(min = 1, max = 255))]
    pub first_name: Option<String>,
    #[garde(length(min = 1, max = 5000))]
    #[omit(short, sort)]
    pub description: Option<String>,
    #[garde(range(min = 0))]
    #[spec(version)]
    pub version: i64,
    #[spec(created_by)]
    pub created_by: Option<String>,
    #[spec(created)]
    pub created: time::PrimitiveDateTime,
    #[spec(modified)]
    pub modified: time::PrimitiveDateTime,
}

#[test]
fn test_repository() {
    let now = time::OffsetDateTime::now_utc();
    let now = time::PrimitiveDateTime::new(now.date(), now.time());
    let _author = Author {
        id: 1,
        last_name: "Usak".into(),
        first_name: Some("Kulisak".into()),
        description: None,
        version: 1,
        created_by: Some("ivan".into()),
        created: now,
        modified: now,
    };

    let _short = AuthorShort {
        id: 1,
        last_name: "Horpach".into(),
        first_name: Some("Kopac".into()),
    };

    let _create = CreateAuthor {
        last_name: "Usak".into(),
        first_name: Some("Kulisak".into()),
        description: None,
        created_by: None,
    };

    let _create = UpdateAuthor {
        id: 1,
        last_name: "Usak".into(),
        first_name: Some("Kulisak".into()),
        description: None,
        version: 1,
    };

    assert!(!VALID_ORDER_FIELDS.contains(&"description"));
    for n in &["id", "created", "modified"] {
        assert!(VALID_ORDER_FIELDS.contains(n));
    }
    let mut order_fields: HashSet<&str> = HashSet::with_capacity(VALID_ORDER_FIELDS.len());
    order_fields.extend(VALID_ORDER_FIELDS);
    assert_eq!(order_fields.len(), VALID_ORDER_FIELDS.len())
}
