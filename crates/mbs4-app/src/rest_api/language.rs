use crate::{crud_api, value_router};
use mbs4_dal::language::{
    CreateLanguage, Language, LanguageRepository, LanguageShort, UpdateLanguage,
};

crud_api!(Language);

value_router!();
