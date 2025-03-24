use std::str::FromStr;

use garde::Validate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Validate, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[garde(transparent)]
pub struct ValidEmail(#[garde(email)] String);

impl FromStr for ValidEmail {
    type Err = garde::Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let email = ValidEmail(s.to_string());
        email.validate()?;
        Ok(email)
    }
}

impl AsRef<str> for ValidEmail {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
