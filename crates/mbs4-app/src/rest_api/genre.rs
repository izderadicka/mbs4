use crate::{crud_api, publish_api_docs, value_router};
#[cfg_attr(not(feature = "openapi"), allow(unused_imports))]
use mbs4_dal::genre::{CreateGenre, Genre, GenreRepository, GenreShort, UpdateGenre};

publish_api_docs!();
crud_api!(Genre);

value_router!();
