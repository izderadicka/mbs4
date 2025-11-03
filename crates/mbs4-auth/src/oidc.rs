use std::collections::HashSet;

use anyhow::anyhow;
use mbs4_types::{claim::UserClaim, oidc::OIDCProviderConfig};
use openidconnect::{
    core::{
        CoreAuthenticationFlow, CoreClient, CoreGenderClaim, CoreJweContentEncryptionAlgorithm,
        CoreJwsSigningAlgorithm, CoreProviderMetadata,
    },
    AccessToken, AccessTokenHash, AuthorizationCode, ClientId, ClientSecret, CsrfToken,
    EmptyAdditionalClaims, EndpointMaybeSet, EndpointNotSet, EndpointSet, IdToken, IdTokenClaims,
    IssuerUrl, Nonce, OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl,
    RefreshToken, Scope, TokenResponse,
};
use serde::{Deserialize, Serialize};
use tracing::debug;
use url::Url;

use crate::{error::Result, Error};
type ConfiguredClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

#[derive(Debug, Clone)]
pub struct OIDCClient {
    client: ConfiguredClient,
    http_client: reqwest::Client,
}

impl OIDCClient {
    pub async fn discover(
        provider: &OIDCProviderConfig,
        redirect_url: impl Into<String>,
    ) -> Result<Self> {
        let http_client = reqwest::ClientBuilder::new()
            // Following redirects opens the client up to SSRF vulnerabilities.
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let provider_metadata = CoreProviderMetadata::discover_async(
            IssuerUrl::new(provider.issuer_url.clone())?,
            &http_client,
        )
        .await?;

        let client = CoreClient::from_provider_metadata(
            provider_metadata,
            ClientId::new(provider.client_id.clone()),
            provider
                .client_secret
                .as_ref()
                .map(|s| ClientSecret::new(s.to_string())),
        )
        // Set the URL the user will be redirected to after the authorization process.
        .set_redirect_uri(RedirectUrl::new(redirect_url.into())?);

        debug!("Discovered OIDC provider: {:?}", client);

        Ok(Self {
            client,
            http_client,
        })
    }

    pub fn auth_url(&self) -> (Url, OIDCSecrets) {
        self.auth_url_with_scopes(None::<String>)
    }
    pub fn auth_url_with_scopes(
        &self,
        scopes: impl IntoIterator<Item = impl Into<String>>,
    ) -> (Url, OIDCSecrets) {
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let mut url_builder = self
            .client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            // Set the PKCE code challenge.
            .set_pkce_challenge(pkce_challenge);

        // Set the desired scopes.
        for scope in scopes {
            url_builder = url_builder.add_scope(Scope::new(scope.into()));
        }
        let (url, csrf_token, nonce) = url_builder.url();
        debug!("Generated auth URL: {}", url);
        (
            url,
            OIDCSecrets {
                csrf_token,
                nonce,
                pkce_verifier,
            },
        )
    }

    pub async fn token(
        &self,
        code: String,
        state: String,
        secrets: OIDCSecrets,
    ) -> Result<IDToken> {
        if &state != secrets.csrf_token.secret() {
            return Err(anyhow!("CSRF token mismatch"));
        }
        let token_response = self
            .client
            .exchange_code(AuthorizationCode::new(code))?
            // Set the PKCE code verifier.
            .set_pkce_verifier(secrets.pkce_verifier)
            .request_async(&self.http_client)
            .await?;

        // Extract the ID token claims after verifying its authenticity and nonce.
        let id_token = token_response
            .id_token()
            .ok_or_else(|| anyhow!("Server did not return an ID token"))?;
        let id_token_verifier = self.client.id_token_verifier();
        let claims = id_token.claims(&id_token_verifier, &secrets.nonce)?;

        // Verify the access token hash to ensure that the access token hasn't been substituted for
        // another user's.
        if let Some(expected_access_token_hash) = claims.access_token_hash() {
            let actual_access_token_hash = AccessTokenHash::from_token(
                token_response.access_token(),
                id_token.signing_alg()?,
                id_token.signing_key(&id_token_verifier)?,
            )?;
            if actual_access_token_hash != *expected_access_token_hash {
                return Err(anyhow!("Invalid access token"));
            }
            return Ok(IDToken {
                claims: claims.clone(),
                id_token: id_token.clone(),
                access_token: Some(token_response.access_token().clone()),
                refresh_token: token_response.refresh_token().cloned(),
            });
        }
        Err(anyhow!("Access token hash is missing"))
    }
}

pub struct IDToken {
    pub claims: IdTokenClaims<EmptyAdditionalClaims, CoreGenderClaim>,
    pub id_token: IdToken<
        EmptyAdditionalClaims,
        CoreGenderClaim,
        CoreJweContentEncryptionAlgorithm,
        CoreJwsSigningAlgorithm,
    >,
    pub access_token: Option<AccessToken>,
    pub refresh_token: Option<RefreshToken>,
}

impl TryFrom<&IDToken> for UserClaim {
    type Error = Error;

    fn try_from(value: &IDToken) -> std::result::Result<Self, Self::Error> {
        let email = value
            .claims
            .email()
            .as_ref()
            .map(|s| s.to_string())
            .ok_or_else(|| Error::msg("Missing email"))?;
        let username = value
            .claims
            .name()
            .as_ref()
            .and_then(|s| s.get(None))
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                email
                    .split("@")
                    .next()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| email.clone())
            });
        let sub = value.claims.subject().to_string();
        Ok(UserClaim {
            email,
            username,
            sub,
            roles: HashSet::new(),
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OIDCSecrets {
    csrf_token: CsrfToken,
    nonce: Nonce,
    pkce_verifier: PkceCodeVerifier,
}

#[cfg(test)]
mod tests {
    use mbs4_types::oidc::OIDCConfig;
    use tracing_test::traced_test;

    use super::*;

    #[tokio::test]
    #[traced_test]
    async fn test_discovery() {
        let config = OIDCConfig::load_config("../../test-data/samples/oidc-config-sample").unwrap();
        let config = config.get_provider("google").unwrap();
        let client = OIDCClient::discover(config, "http://localhost:3000")
            .await
            .unwrap();
        assert_eq!(client.client.client_id().as_str(), "ABCDE");
        let (url, _secrets) = client.auth_url_with_scopes(["email", "profile"]);
        assert!(url.to_string().contains("email"));
    }
}
