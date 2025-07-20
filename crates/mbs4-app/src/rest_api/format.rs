use crate::{crud_api, value_router};
use mbs4_dal::format::{CreateFormat, FormatRepository, UpdateFormat};

crud_api!(Format);

value_router!();
