use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use mbs4_types::claim::TimeLimited;
use serde::de::DeserializeOwned;

use crate::error::Result;

struct Keys {
    encoding: EncodingKey,
    decoding: DecodingKey,
}

impl Keys {
    pub fn new(secret: impl AsRef<[u8]>) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret.as_ref()),
            decoding: DecodingKey::from_secret(secret.as_ref()),
        }
    }
}

pub struct TokenManager {
    keys: Keys,
    default_validity: std::time::Duration,
    header: Header,
    validation: Validation,
}

impl TokenManager {
    pub fn new(secret: impl AsRef<[u8]>, default_validity: std::time::Duration) -> Self {
        let validation = Validation::default();
        let header = Header::default();
        Self {
            keys: Keys::new(secret),
            default_validity,
            header,
            validation,
        }
    }

    pub fn issue(&self, mut claims: impl serde::Serialize + TimeLimited) -> Result<String> {
        let now = std::time::SystemTime::now();
        let validity = now + self.default_validity;
        claims.set_validity(validity);
        let token = encode(&self.header, &claims, &self.keys.encoding)?;
        Ok(token)
    }

    #[cfg(test)]
    pub fn issue_expired(&self, mut claims: impl serde::Serialize + TimeLimited) -> Result<String> {
        let now = std::time::SystemTime::now();
        let validity = now - self.default_validity;
        claims.set_validity(validity);
        let token = encode(&self.header, &claims, &self.keys.encoding)?;
        Ok(token)
    }

    pub fn validate<T>(&self, token: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let data = decode::<T>(token, &self.keys.decoding, &self.validation)?;
        Ok(data.claims)
    }

    pub fn default_validity(&self) -> std::time::Duration {
        self.default_validity
    }
}

#[cfg(test)]
mod tests {
    use mbs4_types::claim::ApiClaim;

    use super::*;

    #[test]
    fn test_token() {
        let claim = ApiClaim {
            exp: 0,
            sub: "123".to_string(),
            roles: ["admin".into(), "user".into()].into(),
        };
        let manager = TokenManager::new("secret", std::time::Duration::from_secs(3600));
        let token = manager.issue(claim).unwrap();
        let res = manager.validate::<ApiClaim>(&token);
        assert!(res.is_ok());
        let claim = res.unwrap();
        assert_eq!(claim.sub, "123");
        assert!(claim.check_validity());
    }

    #[test]
    fn test_token_expiration() {
        let claim = ApiClaim {
            exp: 0,
            sub: "123".to_string(),
            roles: ["admin".into(), "user".into()].into(),
        };
        let manager = TokenManager::new("secret", std::time::Duration::from_secs(3600));
        let token = manager.issue_expired(claim).unwrap();
        let res = manager.validate::<ApiClaim>(&token);
        assert!(res.is_err());
        let err = res.unwrap_err();

        match err
            .root_cause()
            .downcast_ref::<jsonwebtoken::errors::Error>()
        {
            Some(e) => assert!(matches!(
                e.kind(),
                jsonwebtoken::errors::ErrorKind::ExpiredSignature
            )),
            None => panic!("Unexpected error: {}", err),
        }
    }
}
