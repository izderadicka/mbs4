pub mod auth;
pub mod error;
pub mod state;
pub mod user;

#[macro_export]
macro_rules! repository_from_request {
    ($repo:ty) => {
        impl axum::extract::FromRequestParts<$crate::state::AppState> for $repo {
            type Rejection = http::StatusCode;

            fn from_request_parts(
                _parts: &mut http::request::Parts,
                state: &$crate::state::AppState,
            ) -> impl std::future::Future<Output = std::result::Result<Self, Self::Rejection>>
                   + core::marker::Send {
                futures::future::ready(std::result::Result::Ok(<$repo>::new(state.pool().clone())))
            }
        }
    };
}
