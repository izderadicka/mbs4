use quote::{format_ident, quote};
use syn::Data;

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
        let base_fields = data
            .fields
            .iter()
            .filter(|f| f.ident.is_some() && f.ident.as_ref() != Some(&format_ident!("version")));

        let fields = base_fields.clone().map(|f| {
            let mut field = f.clone();
            field.attrs = vec![];
            field.vis = syn::Visibility::Public(syn::token::Pub::default());
            field
        });

        // unwrap is ok as we filter unnamed fields above
        let sortable_fields = fields.clone().map(|f| f.ident.unwrap().to_string());

        let full_struct_name = format_ident!("{}", entity_name);

        let common_fields = quote! {
            pub id: i64,
            #(#fields,)*
        };

        let common_atts = quote! {
            #[derive(Debug, serde::Serialize, serde::Deserialize, Clone, sqlx::FromRow)]
        };

        let full_struct = quote! {
            #common_atts
            pub struct #full_struct_name {
                #common_fields
                pub version: i64,
            }
        };

        let short_struct_name = format_ident!("{}Short", entity_name);
        let short_struct = quote! {
            #common_atts
            pub struct #short_struct_name {
                #common_fields
            }

        };

        let sortable_fields_const = quote! {
            const VALID_ORDER_FIELDS: &[&str] = &["id", #(#sortable_fields),*];
        };

        let repo_impl = quote! {};

        quote! {
            #full_struct
            #short_struct
            #sortable_fields_const
            #repo_impl
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
