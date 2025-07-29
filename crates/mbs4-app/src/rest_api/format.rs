use crate::{crud_api, publish_api_docs, value_router};
#[cfg_attr(not(feature = "openapi"), allow(unused_imports))]
use mbs4_dal::format::{CreateFormat, Format, FormatRepository, FormatShort, UpdateFormat};

publish_api_docs!();
crud_api!(Format);

value_router!();
