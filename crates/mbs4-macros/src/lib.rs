use quote::{format_ident, quote};
use syn::{Data, Token};

#[proc_macro_derive(Repository)]
pub fn repository(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    let name = input.ident.to_string();
    let entity_name = if name.starts_with("Create") {
        name.replace("Create", "")
    } else {
        let e = syn::Error::new(
            input.ident.span(),
            format!("Unexpected name {}, should start with Create", name),
        );
        return e.to_compile_error().into();
    };

    if let Data::Struct(data) = input.data {
        let fields = data
            .fields
            .iter()
            .filter(|f| f.ident.as_ref() != Some(&format_ident!("version")))
            .map(
                (|f| {
                    let mut field = f.clone();
                    field.attrs = vec![];
                    field.vis = syn::Visibility::Public(syn::token::Pub::default());
                    field
                }),
            );
        let full_struct_name = format_ident!("{}", entity_name);
        quote! {
            #[derive(Debug, serde::Serialize, serde::Deserialize, Clone, sqlx::FromRow)]
            pub struct #full_struct_name {
                #(#fields,)*
                pub id: i64,
                pub version: i64,
            }
        }
        .into()
    } else {
        let e = syn::Error::new(
            input.ident.span(),
            format!("Unexpected data type, should be struct"),
        );
        e.to_compile_error().into()
    }
}
