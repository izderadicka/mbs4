use quote::{format_ident, quote};
use syn::{Data, Field, Ident, punctuated::Punctuated};

const OMIT: &str = "omit";
const OMIT_SHORT: &str = "short";
const OMIT_SORT: &str = "sort";

fn prepare_field(f: &Field) -> Field {
    let mut field = f.clone();
    field.attrs = vec![];
    field.vis = syn::Visibility::Public(syn::token::Pub::default());
    field
}

fn filter_field(f: &Field, omit: &str) -> bool {
    for attr in &f.attrs {
        if attr.path().is_ident(OMIT) {
            let params = attr
                .parse_args_with(Punctuated::<Ident, syn::Token![,]>::parse_terminated)
                .unwrap();
            if params.into_iter().any(|n| n == omit) {
                return false;
            }
        }
    }
    true
}

pub fn repository(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    let create_struct_name = input.ident.clone();
    let name = create_struct_name.to_string();
    let entity_name = if name.starts_with("Create") {
        name.replace("Create", "")
    } else {
        let e = syn::Error::new(
            input.ident.span(),
            format!("Unexpected name {}, should start with Create", name),
        );
        return e.to_compile_error().into();
    };
    let table_name = entity_name.to_lowercase();

    if let Data::Struct(data) = input.data {
        let base_fields = data
            .fields
            .iter()
            .filter(|f| f.ident.is_some() && f.ident.as_ref() != Some(&format_ident!("version")));

        let fields = base_fields.clone().map(prepare_field);
        let short_fields = base_fields
            .clone()
            .filter(|f| filter_field(f, OMIT_SHORT))
            .map(prepare_field);

        // unwrap is ok as we filter unnamed fields above
        let sortable_fields = base_fields
            .clone()
            .filter(|f| filter_field(f, OMIT_SORT))
            .map(|f| f.ident.as_ref().unwrap().to_string());
        let mutable_fields_idents = fields.clone().map(|f| f.ident.unwrap()).collect::<Vec<_>>();
        let mutable_fields = mutable_fields_idents
            .iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>();

        let full_struct_name = format_ident!("{}", entity_name);

        let common_atts = quote! {
            #[derive(Debug, serde::Serialize, serde::Deserialize, Clone, sqlx::FromRow)]
        };

        let full_struct = quote! {
            #common_atts
            pub struct #full_struct_name {
                pub id: i64,
                #(#fields,)*
                pub version: i64,
                pub created_by: Option<String>,
                pub created: time::OffsetDateTime,
                pub modified: time::OffsetDateTime,
            }
        };

        let short_struct_name = format_ident!("{}Short", entity_name);
        let short_struct = quote! {
            #common_atts
            pub struct #short_struct_name {
                pub id: i64,
                #(#short_fields,)*
            }

        };

        let sortable_fields_const = quote! {
            const VALID_ORDER_FIELDS: &[&str] = &["id", "created", "modified", #(#sortable_fields),*];
        };

        // REPO ===================================================================
        let repo_name = format_ident!("{}Repository", entity_name);
        let repo_impl_name = format_ident!("{}RepositoryImpl", entity_name);
        let insert_fields = mutable_fields.join(",");
        let placeholders = mutable_fields
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");

        let insert_cmd =
            format!("INSERT INTO {table_name}({insert_fields},version) VALUES ({placeholders},1)");
        let bound_fields = mutable_fields_idents
            .iter()
            .map(|f| {
                quote!(.bind(&payload.#f
                ))
            })
            .collect::<Vec<_>>();

        let update_fields = mutable_fields
            .iter()
            .map(|f| format!("{} = ?", f))
            .collect::<Vec<_>>()
            .join(",");
        let update_cmd = format!(
            "UPDATE {table_name} SET {update_fields},version = ? WHERE id = ? and version = ?"
        );
        let count_cmd = format!("SELECT count(*) FROM {table_name}");
        let select_many_query =
            format!("SELECT id,{insert_fields} FROM {table_name} {{order}} LIMIT ? OFFSET ?");
        let delete_cmd = format!("DELETE FROM {table_name} WHERE id = ?");
        let select_one_query = format!("SELECT * FROM {table_name} WHERE id = ?");

        let repo_impl = quote! {
            use futures::{StreamExt as _, TryStreamExt as _};
            use sqlx::{Acquire as _, Executor as _};

            pub type #repo_name = #repo_impl_name<sqlx::Pool<crate::ChosenDB>>;

            pub struct #repo_impl_name<E> {
                executor: E,
            }

            impl<'c, E> #repo_impl_name<E>
            where
                for<'a> &'a E:
                    sqlx::Executor<'c, Database = crate::ChosenDB> + sqlx::Acquire<'c, Database = crate::ChosenDB>,
            {
                pub fn new(executor: E) -> Self {
                    Self { executor }
                }

                pub async fn create(&self, payload: #create_struct_name) -> crate::error::Result<#full_struct_name> {
                    let result = sqlx::query(#insert_cmd)
                        #(#bound_fields)*
                        .execute(&self.executor)
                        .await?;

                    let id = result.last_insert_rowid();
                    self.get(id).await
                }

                pub async fn update(&self, id: i64, payload: #create_struct_name) -> crate::error::Result<#full_struct_name> {
                    let version = payload.version.ok_or_else(|| {
                        tracing::debug!("No version provided");
                        crate::error::Error::MissingVersion
                    })?;
                    let mut conn = self.executor.acquire().await?;
                    let mut transaction = conn.begin().await?;
                    let result = sqlx::query(
                        #update_cmd,
                    )
                    #(#bound_fields)*
                    .bind(version + 1)
                    .bind(id)
                    .bind(version)
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

                pub async fn count(&self) -> crate::error::Result<u64> {
                    let count: u64 = sqlx::query_scalar(#count_cmd)
                        .fetch_one(&self.executor)
                        .await?;
                    Ok(count)
                }

                pub async fn list_all(&self) -> crate::error::Result<Vec<#short_struct_name>> {
                    self.list(crate::ListingParams::default()).await
                }

                pub async fn list(&self, params: crate::ListingParams) -> crate::error::Result<Vec<#short_struct_name>> {
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
                    Ok(records)
                }

                pub async fn delete(&self, id: i64) -> crate::error::Result<()> {
                    let res = sqlx::query(#delete_cmd)
                        .bind(id)
                        .execute(&self.executor)
                        .await?;

                    if res.rows_affected() == 0 {
                        Err(crate::error::Error::RecordNotFound("Language".to_string()))
                    } else {
                        Ok(())
                    }
                }

                pub async fn get(&self, id: i64) -> crate::error::Result<#full_struct_name> {
                    get(id, &self.executor).await
                }
            }

            async fn get<'c, E>(id: i64, executor: E) -> crate::error::Result<#full_struct_name>
            where
                E: sqlx::Executor<'c, Database = crate::ChosenDB>,
            {
                let record: #full_struct_name = sqlx::query_as!(#full_struct_name, #select_one_query, id)
                    .fetch_one(executor)
                    .await?
                    .into();
                Ok(record)
            }
        };
        // REPO END ===============================================================
        quote! {
            #full_struct
            #short_struct
            #sortable_fields_const

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
