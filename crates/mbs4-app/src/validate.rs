use axum::extract::{FromRequest, Request};
use axum::response::{IntoResponse, Response};
use garde::{Report, Validate};
use http::StatusCode;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::ops::{Deref, DerefMut};

use crate::state::AppState;

#[derive(Debug, Clone, Copy, Default)]
pub struct Garde<E>(pub E);

impl<E> Deref for Garde<E> {
    type Target = E;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<E> DerefMut for Garde<E> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<E: Display> Display for Garde<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl<E> Garde<E> {
    /// Consumes the `Garde` and returns the validated data within.
    ///
    /// This returns the `E` type which represents the data that has been
    /// successfully validated.
    pub fn into_inner(self) -> E {
        self.0
    }
}

#[derive(Debug)]
pub enum ValidationRejection<V, E> {
    /// `Valid` variant captures errors related to the validation logic.
    Valid(V),
    /// `Inner` variant represents potential errors that might occur within the inner extractor.
    Inner(E),
}

impl<V: Display, E: Display> Display for ValidationRejection<V, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationRejection::Valid(errors) => write!(f, "{errors}"),
            ValidationRejection::Inner(error) => write!(f, "{error}"),
        }
    }
}

impl<V: Error + 'static, E: Error + 'static> Error for ValidationRejection<V, E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ValidationRejection::Valid(ve) => Some(ve),
            ValidationRejection::Inner(e) => Some(e),
        }
    }
}

impl<V: serde::Serialize, E: IntoResponse> IntoResponse for ValidationRejection<V, E> {
    fn into_response(self) -> Response {
        match self {
            ValidationRejection::Valid(v) => {
                (StatusCode::UNPROCESSABLE_ENTITY, axum::Json(v)).into_response()
            }
            ValidationRejection::Inner(e) => e.into_response(),
        }
    }
}

/// `GardeRejection` is returned when the `Garde` extractor fails.
///
pub type GardeRejection<E> = ValidationRejection<Report, E>;

impl<E> From<Report> for GardeRejection<E> {
    fn from(value: Report) -> Self {
        Self::Valid(value)
    }
}

impl<Extractor, T> FromRequest<AppState> for Garde<Extractor>
where
    T: Validate<Context = ()>,
    Extractor: Deref<Target = T> + FromRequest<AppState>,
{
    type Rejection = GardeRejection<<Extractor as FromRequest<AppState>>::Rejection>;

    async fn from_request(req: Request, state: &AppState) -> Result<Self, Self::Rejection> {
        let inner = Extractor::from_request(req, state)
            .await
            .map_err(GardeRejection::Inner)?;

        inner.deref().validate()?;
        Ok(Garde(inner))
    }
}

// impl<State, Extractor, Context> FromRequestParts<State> for Garde<Extractor>
// where
//     State: Send + Sync,
//     Context: Send + Sync + FromRef<State>,
//     Extractor: HasValidate + FromRequestParts<State>,
//     <Extractor as HasValidate>::Validate: garde::Validate<Context = Context>,
// {
//     type Rejection = GardeRejection<<Extractor as FromRequestParts<State>>::Rejection>;

//     async fn from_request_parts(parts: &mut Parts, state: &State) -> Result<Self, Self::Rejection> {
//         let context: Context = FromRef::from_ref(state);
//         let inner = Extractor::from_request_parts(parts, state)
//             .await
//             .map_err(GardeRejection::Inner)?;
//         inner.get_validate().validate_with(&context)?;
//         Ok(Garde(inner))
//     }
// }

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use garde::{Path, Report};
//     use std::error::Error;
//     use std::io;

//     const GARDE: &str = "garde";

//     #[test]
//     fn garde_deref_deref_mut_into_inner() {
//         let mut inner = String::from(GARDE);
//         let mut v = Garde(inner.clone());
//         assert_eq!(&inner, v.deref());
//         inner.push_str(GARDE);
//         v.deref_mut().push_str(GARDE);
//         assert_eq!(&inner, v.deref());
//         println!("{}", v);
//         assert_eq!(inner, v.into_inner());
//     }

//     #[test]
//     fn display_error() {
//         // GardeRejection::Valid Display
//         let mut report = Report::new();
//         report.append(Path::empty(), garde::Error::new(GARDE));
//         let s = report.to_string();
//         let vr = GardeRejection::<String>::Valid(report);
//         assert_eq!(vr.to_string(), s);

//         // GardeRejection::Inner Display
//         let inner = String::from(GARDE);
//         let vr = GardeRejection::<String>::Inner(inner.clone());
//         assert_eq!(inner.to_string(), vr.to_string());

//         // GardeRejection::Valid Error
//         let mut report = Report::new();
//         report.append(Path::empty(), garde::Error::new(GARDE));
//         let vr = GardeRejection::<io::Error>::Valid(report);
//         assert!(matches!(vr.source(), Some(source) if source.downcast_ref::<Report>().is_some()));

//         // GardeRejection::Valid Error
//         let vr = GardeRejection::<io::Error>::Inner(io::Error::new(io::ErrorKind::Other, GARDE));
//         assert!(
//             matches!(vr.source(), Some(source) if source.downcast_ref::<io::Error>().is_some())
//         );
//     }
// }
