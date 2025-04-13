mod entity_repo;
mod value_repo;

#[proc_macro_derive(ValueRepository)]
pub fn value_repo(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    value_repo::repository(input)
}

#[proc_macro_derive(Repository, attributes(omit, spec, garde))]
pub fn entity_repo(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    entity_repo::repository(input)
}
