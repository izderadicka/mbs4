use quote::quote;

#[proc_macro_derive(Repository)]
pub fn repository(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    quote! {}.into()
}
