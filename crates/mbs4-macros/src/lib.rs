mod entity_repo;

#[proc_macro_derive(Repository, attributes(omit, spec, garde))]
pub fn entity_repo(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    entity_repo::repository(input)
}
