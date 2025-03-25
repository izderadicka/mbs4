use std::str::FromStr;

use garde::Validate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Validate, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[garde(transparent)]
pub struct ValidEmail(#[garde(email)] String);

#[cfg(feature = "e2e-tests")]
impl ValidEmail {
    pub fn cheat(email: String) -> Self {
        ValidEmail(email)
    }
}

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

#[cfg(test)]
mod tests {
    use fake::Fake as _;
    use quickcheck::Arbitrary;
    use quickcheck_macros::quickcheck;

    use super::*;

    impl Arbitrary for ValidEmail {
        fn arbitrary(_g: &mut quickcheck::Gen) -> Self {
            let email: String = fake::faker::internet::en::SafeEmail().fake();
            ValidEmail(email)
        }
    }

    //useless but good exercise on quickcheck
    #[quickcheck]
    fn test_valid_email_arbitrary(valid_email: ValidEmail) {
        println!("email: {}", valid_email.as_ref());
        assert!(valid_email.validate().is_ok());
    }

    #[test]
    fn test_valid_email() {
        let email = ValidEmail::from_str("admin@localhost").unwrap();
        assert_eq!(email.as_ref(), "admin@localhost");
    }

    #[test]
    fn test_invalid_email() {
        let email = ValidEmail::from_str("admin");
        assert!(email.is_err());

        // cheet on creation
        let email = ValidEmail("admin".to_string());
        assert!(email.validate().is_err());
    }
}
