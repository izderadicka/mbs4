use std::collections::HashMap;

use serde::Deserialize;

use crate::error::Result;

#[derive(Debug, Deserialize, Clone)]
pub struct OIDCProviderConfig {
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OIDCConfig {
    providers: HashMap<String, OIDCProviderConfig>,
}

impl OIDCConfig {
    pub fn get_provider(&self, name: &str) -> Option<&OIDCProviderConfig> {
        self.providers.get(name)
    }

    pub fn available_providers(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    pub fn load_config(file_source: &str) -> Result<Self> {
        let config = config::Config::builder()
            .add_source(config::File::with_name(file_source))
            .build()?;
        let config = config.try_deserialize::<OIDCConfig>()?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config() {
        let config = OIDCConfig::load_config("../../test-data/oidc-config").unwrap();
        assert_eq!(config.available_providers().len(), 3);
        let discord = config.get_provider("google").unwrap();
        assert_eq!(discord.client_id, "ABCDE");
        assert_eq!(discord.client_secret, Some("12345".into()))
    }
}
