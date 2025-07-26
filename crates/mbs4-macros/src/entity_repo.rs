use quote::{format_ident, quote};
use syn::{Attribute, Data, Field, Ident, punctuated::Punctuated};

const OMIT: &str = "omit";
const OMIT_SHORT: &str = "short";
const OMIT_SORT: &str = "sort";
const SPEC: &str = "spec";
const SPEC_ID: &str = "id";
const SPEC_CREATED_BY: &str = "created_by";
const SPEC_CREATED: &str = "created";
const SPEC_MODIFIED: &str = "modified";
const SPEC_VERSION: &str = "version";
const GARDE: &str = "garde";

fn prepare_field(f: &Field) -> Field {
    let mut field = f.clone();
    field.attrs = vec![];
    field.vis = syn::Visibility::Public(syn::token::Pub::default());
    field
}

fn prepare_input_field(f: &Field) -> Field {
    let mut field = f.clone();
    field.attrs = f
        .attrs
        .clone()
        .into_iter()
        .filter(|a| a.path().is_ident(GARDE))
        .collect();
    field.vis = syn::Visibility::Public(syn::token::Pub::default());
    field
}

fn params_contains(attr: &Attribute, name: &str) -> bool {
    attr.parse_args_with(Punctuated::<Ident, syn::Token![,]>::parse_terminated)
        .unwrap()
        .into_iter()
        .any(|n| n == name)
}

// fn params_contains_any(attr: &Attribute, names: &[&str]) -> bool {
//     attr.parse_args_with(Punctuated::<Ident, syn::Token![,]>::parse_terminated)
//         .unwrap()
//         .into_iter()
//         .any(|n| names.iter().any(|name| n == *name))
// }

fn field_has_attr_with_value(f: &Field, attr_name: &str, value: &str) -> bool {
    for attr in &f.attrs {
        if attr.path().is_ident(attr_name) && params_contains(&attr, value) {
            return true;
        }
    }
    return false;
}

fn special_field_name<'a>(
    mut fields: impl Iterator<Item = &'a Field>,
    spec_param: &str,
) -> Option<String> {
    fields
        .find(|f| field_has_attr_with_value(f, SPEC, spec_param))
        .map(|f| f.ident.as_ref().unwrap().to_string())
}

// fn type_is_datetime(ty: &Type) -> bool {
//     match ty {
//         Type::Path(p) => p.path.segments.iter().any(|s| {
//             let name = s.ident.to_string().to_lowercase();
//             name.contains("datetime")
//         }),
//         _ => false,
//     }
// }

fn filter_field(f: &Field, omit: Option<&str>, keep_spec: &[&str]) -> bool {
    for attr in &f.attrs {
        if omit.is_some() && attr.path().is_ident(OMIT) {
            let omit = omit.unwrap();
            if params_contains(attr, omit) {
                return false;
            }
        } else if attr.path().is_ident(SPEC) {
            if keep_spec.is_empty() {
                return false;
            }
            let mut keep = false;
            for name in keep_spec {
                if params_contains(attr, name) {
                    keep = true;
                    break;
                }
            }
            if !keep {
                return false;
            }
        }
    }
    true
}

pub fn repository(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);

    if let Data::Struct(data) = input.data {
        let entity_ident = input.ident.clone();
        let entity_name = entity_ident.to_string();

        let table_name = entity_name.to_lowercase();
        let base_fields = data.fields.iter().filter(|f| f.ident.is_some());

        let common_input_atts = quote! {
            #[derive(Debug,  serde::Deserialize, Clone, garde::Validate)]
            #[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
        };

        let create_fields: Vec<_> = base_fields
            .clone()
            .filter(|f| filter_field(f, None, &[SPEC_CREATED_BY]))
            .map(prepare_input_field)
            .collect();

        let create_struct_name = format_ident!("Create{entity_name}");
        let create_struct = quote! {
            #common_input_atts
            #[garde(allow_unvalidated)]
            pub struct #create_struct_name {
                #(#create_fields,)*
            }
        };

        let update_fields: Vec<_> = base_fields
            .clone()
            .filter(|f| filter_field(f, None, &[SPEC_ID, SPEC_VERSION]))
            .map(prepare_input_field)
            .collect();
        let update_struct_name = format_ident!("Update{entity_name}");
        let update_struct = quote! {
            #common_input_atts
            #[garde(allow_unvalidated)]
            pub struct #update_struct_name {
                #(#update_fields,)*
            }
        };

        let short_fields: Vec<_> = base_fields
            .clone()
            .filter(|f| filter_field(f, Some(OMIT_SHORT), &[SPEC_ID]))
            .map(prepare_field)
            .collect();

        let short_struct_name = format_ident!("{}Short", entity_name);
        let row_impl_fields = short_fields.iter().map(|f| {
            let ident = f.ident.as_ref().unwrap();
            let column_name = format!("{}_{}", table_name, ident.to_string());
            quote! { #ident:row.try_get(#column_name)?}
        });
        let from_row_impl = quote! {
            impl crate::FromRowPrefixed<'_, crate::ChosenRow> for #short_struct_name  {
                fn from_row_prefixed(row: &crate::ChosenRow) -> crate::error::Result<Self, sqlx::Error> {
                    use sqlx::Row;
                    Ok(#short_struct_name  {
                        #(#row_impl_fields,)*
                    })
                }
            }

        };
        let short_struct = quote! {
                    #[derive(Debug, Serialize, Deserialize,Clone, sqlx::FromRow)]
        #[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
                    pub struct #short_struct_name {
                        #(#short_fields,)*
                    }
                    #from_row_impl

                };

        // unwrap is ok as we filter unnamed fields above
        let sortable_fields = base_fields
            .clone()
            .filter(|f| filter_field(f, Some(OMIT_SORT), &[SPEC_ID, SPEC_CREATED, SPEC_MODIFIED]))
            .map(|f| f.ident.as_ref().unwrap().to_string());

        let sortable_fields_const = quote! {
            const VALID_ORDER_FIELDS: &[&str] = &[#(#sortable_fields),*];
        };

        // REPO ===================================================================
        let repo_name = format_ident!("{}Repository", entity_name);
        let repo_impl_name = format_ident!("{}RepositoryImpl", entity_name);

        let types_def = quote! {
            use futures::{StreamExt as _, TryStreamExt as _};
            use sqlx::{Acquire as _, Executor as _};

            pub type #repo_name = #repo_impl_name<sqlx::Pool<crate::ChosenDB>>;

            pub struct #repo_impl_name<E> {
                executor: E,
            }
        };

        // GET =====================================================================

        let select_one_query = format!("SELECT * FROM {table_name} WHERE id = ?");
        let select_one_query_ident: Ident = format_ident!("SELECT_ONE_QUERY");
        let select_one_query_const = quote! {
            const #select_one_query_ident: &str = #select_one_query;
        };

        let get_one_fn = quote! {
            async fn get<'c, E>(id: i64, executor: E) -> crate::error::Result<#entity_ident>
            where
                E: sqlx::Executor<'c, Database = crate::ChosenDB>,
            {
                let record = sqlx::query_as::<_, #entity_ident>(#select_one_query_ident)
                    .bind(id)
                    .fetch_one(executor)
                    .await?
                    .into();
                Ok(record)
            }
        };

        // SPECIAL FIELDS ==========================================================

        let version_field = special_field_name(base_fields.clone(), SPEC_VERSION);
        let created_field = special_field_name(base_fields.clone(), SPEC_CREATED);
        let modified_field = special_field_name(base_fields.clone(), SPEC_MODIFIED);

        // CREATE ==================================================================

        let insert_fields_idents: Vec<_> = create_fields
            .iter()
            .map(|f| f.ident.as_ref().unwrap())
            .collect();
        let mut insert_fields: Vec<_> =
            insert_fields_idents.iter().map(|i| i.to_string()).collect();

        let mut placeholders = insert_fields.iter().map(|_| "?").collect::<Vec<_>>();

        let mut bound_fields_insert = insert_fields_idents
            .iter()
            .map(|f| {
                quote!(.bind(&payload.#f
                ))
            })
            .collect::<Vec<_>>();

        let now_def = if created_field.is_some() || modified_field.is_some() {
            quote!(
                let now = time::OffsetDateTime::now_utc();
                let now = time::PrimitiveDateTime::new(now.date(), now.time());
            )
        } else {
            quote!()
        };

        if let Some(created) = created_field {
            insert_fields.push(created);
            placeholders.push("?".into());
            bound_fields_insert.push({
                let now = format_ident!("now");
                quote!(.bind(#now))
            })
        }

        if let Some(ref modified) = modified_field {
            insert_fields.push(modified.into());
            placeholders.push("?".into());
            bound_fields_insert.push({
                let now = format_ident!("now");
                quote!(.bind(#now))
            })
        }

        if let Some(ref version) = version_field {
            insert_fields.push(version.into());
            placeholders.push("1".into())
        }

        let placeholders = placeholders.join(",");
        let insert_fields = insert_fields.join(",");

        let insert_query =
            format!("INSERT INTO {table_name}({insert_fields}) VALUES ({placeholders})");
        let insert_query_ident: Ident = format_ident!("INSERT_QUERY");
        let insert_query_const = quote! {
            const #insert_query_ident: &str = #insert_query;
        };
        let create_fn = quote!(
            pub async fn create(&self, payload: #create_struct_name) -> crate::error::Result<#entity_ident> {
                #now_def
                let result = sqlx::query(#insert_query_ident)
                    #(#bound_fields_insert)*
                    .execute(&self.executor)
                    .await?;

                let id = result.last_insert_rowid();
                self.get(id).await
            }

        );

        // UPDATE ==================================================================
        let update_fields_idents = base_fields
            .clone()
            .filter(|f| filter_field(f, None, &[]))
            .map(|f| f.ident.as_ref().unwrap());
        let mut update_fields = update_fields_idents
            .clone()
            .map(|f| format!("{} = ?", f))
            .collect::<Vec<_>>();
        if let Some(ref modified) = modified_field {
            update_fields.push(format!("{modified} = ?"));
        }

        let where_clause = if let Some(ref version) = version_field {
            update_fields.push(format!("{version} = ?"));
            format!("{version}=? and")
        } else {
            "".into()
        };
        let update_fields = update_fields.join(",");
        let update_query_ident: Ident = format_ident!("UPDATE_QUERY");
        let update_query =
            format!("UPDATE {table_name} SET {update_fields} WHERE {where_clause} id = ?");
        let update_query_const = quote! {
            const #update_query_ident: &str = #update_query;
        };

        let mut bound_fields_update: Vec<_> = update_fields_idents
            .map(|name| quote!(.bind(payload.#name)))
            .collect();

        let now_def = if modified_field.is_some() {
            bound_fields_update.push(quote!(.bind(now)));
            quote!(
                let now = time::OffsetDateTime::now_utc();
                let now = time::PrimitiveDateTime::new(now.date(), now.time());
            )
        } else {
            quote!()
        };
        let version_def = if let Some(ref version) = version_field {
            let version_ident = format_ident!("{version}");
            bound_fields_update.push(quote!(.bind(#version_ident + 1)));
            bound_fields_update.push(quote!(.bind(#version_ident)));
            quote!(let version = payload.version;)
        } else {
            quote!()
        };

        let update_fn = quote!(
            pub async fn update(&self, id: i64, payload: #update_struct_name) -> crate::error::Result<#entity_ident> {
                #version_def
                #now_def
                if payload.id != id {
                    return Err(crate::Error::InvalidEntity(
                    "Entity id mismatch".into(),
                ));
                }
                let mut conn = self.executor.acquire().await?;
                let mut transaction = conn.begin().await?;
                let result = sqlx::query(
                    #update_query_ident,
                )
                #(#bound_fields_update)*
                .bind(id)
                .execute(&mut *transaction)
                .await?;

                if result.rows_affected() == 0 {
                    Err(crate::error::Error::FailedUpdate { id, version })
                } else {
                    let record = get(id, &mut *transaction).await?;
                    transaction.commit().await?;
                    Ok(record)
                }
            }
        );

        // COUNT ======================================================================

        let count_query = format!("SELECT count(*) FROM {table_name}");
        let count_query_ident: Ident = format_ident!("COUNT_QUERY");
        let count_query_const = quote! {
            const #count_query_ident: &str = #count_query;
        };
        let count_fn = quote! {
            pub async fn count(&self) -> crate::error::Result<u64> {
                let count: u64 = sqlx::query_scalar(#count_query_ident)
                    .fetch_one(&self.executor)
                    .await?;
                Ok(count)
            }
        };

        // LIST ======================================================================
        let short_fields_names = short_fields
            .iter()
            .map(|f| f.ident.as_ref().unwrap().to_string())
            .collect::<Vec<_>>()
            .join(",");
        let select_many_query =
            format!("SELECT {short_fields_names} FROM {table_name} {{order}} LIMIT ? OFFSET ?");
        let select_many_query_ident: Ident = format_ident!("SELECT_MANY_QUERY");
        let select_many_query_const = quote! {
            const #select_many_query_ident: &str = #select_many_query;
        };

        let list_fn = quote! {
            pub async fn list(&self, params: crate::ListingParams) -> crate::error::Result<crate::Batch<#short_struct_name>> {
                let order = params.ordering(VALID_ORDER_FIELDS)?;
                let records = sqlx::query_as::<_, #short_struct_name>(&format!(
                    #select_many_query
                ))
                .bind(params.limit)
                .bind(params.offset)
                .fetch(&self.executor)
                .take(crate::MAX_LIMIT)
                .try_collect::<Vec<_>>()
                .await?;
                let count = self.count().await?;
                Ok(crate::Batch{
                    offset: params.offset,
                    limit: params.limit,
                    rows: records,
                    total: count,
                })
            }
        };

        // DELETE ======================================================================
        let delete_query = format!("DELETE FROM {table_name} WHERE id = ?");
        let delete_query_ident: Ident = format_ident!("DELETE_QUERY");
        let delete_query_const = quote! {
            const #delete_query_ident: &str = #delete_query;
        };
        let delete_fn = quote! {
            pub async fn delete(&self, id: i64) -> crate::error::Result<()> {
                let res = sqlx::query(#delete_query_ident)
                    .bind(id)
                    .execute(&self.executor)
                    .await?;

                if res.rows_affected() == 0 {
                    Err(crate::error::Error::RecordNotFound("Language".to_string()))
                } else {
                    Ok(())
                }
            }
        };

        let repo_impl = quote! {
            impl<'c, E> #repo_impl_name<E>
            where
                for<'a> &'a E:
                    sqlx::Executor<'c, Database = crate::ChosenDB> + sqlx::Acquire<'c, Database = crate::ChosenDB>,
            {
                pub fn new(executor: E) -> Self {
                    Self { executor }
                }

                #create_fn
                #update_fn
                #count_fn
                #list_fn
                #delete_fn

                pub async fn list_all(&self) -> crate::error::Result<Vec<#short_struct_name>> {
                    self.list(crate::ListingParams::new_unpaged()).await.map(|b| b.rows)
                }

                pub async fn get(&self, id: i64) -> crate::error::Result<#entity_ident> {
                    get(id, &self.executor).await
                }
             }

            #get_one_fn
        };
        // REPO END ===============================================================
        quote! {
            #create_struct
            #update_struct
            #short_struct

            #sortable_fields_const
            #select_one_query_const
            #insert_query_const
            #update_query_const
            #count_query_const
            #select_many_query_const
            #delete_query_const

            #types_def
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
