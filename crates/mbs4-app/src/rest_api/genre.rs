use crate::{crud_api, value_router};
use mbs4_dal::genre::{CreateGenre, GenreRepository, UpdateGenre};

crud_api!(Genre);

value_router!();
