use jsonwebtoken::{DecodingKey, EncodingKey};

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
}

impl TokenManager {
    pub fn new(secret: impl AsRef<[u8]>, default_validity: std::time::Duration) -> Self {
        Self {
            keys: Keys::new(secret),
            default_validity,
        }
    }

    // pub fn issue
}
