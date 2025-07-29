use crate::{crud_api, publish_api_docs, value_router};
#[cfg_attr(not(feature = "openapi"), allow(unused_imports))]
use mbs4_dal::language::{
    CreateLanguage, Language, LanguageRepository, LanguageShort, UpdateLanguage,
};

publish_api_docs!();
crud_api!(Language);

value_router!();
