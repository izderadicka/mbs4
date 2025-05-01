use crate::{crud_api, value_router};
use mbs4_dal::language::{CreateLanguage, LanguageRepository, UpdateLanguage};

crud_api!(LanguageRepository, CreateLanguage, UpdateLanguage);

value_router!();
