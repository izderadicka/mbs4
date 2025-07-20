use crate::{crud_api, value_router};
use mbs4_dal::format::{CreateFormat, Format, FormatRepository, FormatShort, UpdateFormat};

crud_api!(Format);

value_router!();
