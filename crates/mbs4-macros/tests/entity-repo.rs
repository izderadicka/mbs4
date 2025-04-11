use garde::Validate;
use mbs4_macros::EntityRepository;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime as Datetime;

#[derive(Debug, Serialize, Deserialize, Clone, Validate, EntityRepository)]
pub struct CreateAuthor {
    #[garde(length(min = 1, max = 255))]
    last_name: String,
    #[garde(length(min = 1, max = 255))]
    first_name: Option<String>,
    #[garde(length(min = 1, max = 5000))]
    #[omit(short, sort)]
    description: Option<String>,
    #[garde(range(min = 0))]
    version: Option<i64>,
}

#[test]
fn test_repository() {
    let now = Datetime::now_utc();
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

    assert!(!VALID_ORDER_FIELDS.contains(&"description"))
}
