use base64::Engine;
use hmac::Mac;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use mbs4_types::claim::TimeLimited;
use rand::{rng, RngCore};
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

type HmacSha256 = hmac::Hmac<sha2::Sha256>;

pub struct TokenManager {
    keys: Keys,
    token_retrieval_secret: Vec<u8>,
    default_validity: std::time::Duration,
    header: Header,
    validation: Validation,
}

impl TokenManager {
    const RETRIEVAL_TOKEN_VALIDITY_SECS: u64 = 5 * 60;
    pub fn new(
        secret: impl AsRef<[u8]>,
        token_retrieval_secret: impl AsRef<[u8]>,
        default_validity: std::time::Duration,
    ) -> Self {
        let validation = Validation::default();
        let header = Header::default();
        Self {
            keys: Keys::new(secret),
            token_retrieval_secret: token_retrieval_secret.as_ref().to_vec(),
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

    pub fn create_tr_token(&self) -> Result<String> {
        let mut mac = HmacSha256::new_from_slice(&self.token_retrieval_secret)?;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let validity = timestamp + Self::RETRIEVAL_TOKEN_VALIDITY_SECS;
        let mut msg = [0u8; 8 + 32 + 32];
        msg[0..8].copy_from_slice(&validity.to_be_bytes());
        let mut rng = rng();
        rng.fill_bytes(&mut msg[8..40]);
        mac.update(&msg[0..40]);
        let sig = mac.finalize().into_bytes();
        msg[40..].copy_from_slice(&sig);
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(msg);

        Ok(token)
    }
    pub fn validate_tr_token(&self, token: &str) -> Result<()> {
        let msg = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(token.as_bytes())?;
        if msg.len() != 8 + 32 + 32 {
            return Err(anyhow::anyhow!("Invalid token length"));
        }
        let timestamp = u64::from_be_bytes(msg[0..8].try_into().unwrap());
        let mut mac = HmacSha256::new_from_slice(&self.token_retrieval_secret)?;
        mac.update(&msg[0..40]);
        mac.verify_slice(&msg[40..])?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        if now > timestamp {
            return Err(anyhow::anyhow!("Token expired"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use mbs4_types::claim::{ApiClaim, Role};

    use super::*;

    fn dummy_claim() -> ApiClaim {
        ApiClaim {
            exp: 0,
            iat: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            sub: "123".to_string(),
            roles: [Role::Admin, Role::Trusted].into(),
        }
    }

    #[test]
    fn test_token() {
        let claim = dummy_claim();
        let manager = TokenManager::new("secret", "secret2", std::time::Duration::from_secs(3600));
        let token = manager.issue(claim).unwrap();
        let res = manager.validate::<ApiClaim>(&token);
        assert!(res.is_ok());
        let claim = res.unwrap();
        assert_eq!(claim.sub, "123");
        assert!(claim.check_validity());
    }

    #[test]
    fn test_token_expiration() {
        let claim = dummy_claim();
        let manager = TokenManager::new("secret", "secret2", std::time::Duration::from_secs(3600));
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

    #[test]
    fn test_tr_token() {
        let manager = TokenManager::new("secret", "secret2", std::time::Duration::from_secs(3600));
        let token = manager.create_tr_token().unwrap();
        manager.validate_tr_token(&token).unwrap();
    }
}
