use crate::{crud_api, value_router};
use mbs4_dal::genre::{CreateGenre, GenreRepository, UpdateGenre};

crud_api!(GenreRepository, CreateGenre, UpdateGenre);

value_router!();
