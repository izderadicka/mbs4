use crate::error::{ApiError, ApiResult};
use garde::Validate;
use mbs4_dal::{Batch, ListingParams};
use serde::Serialize;

#[derive(Debug, Clone, Validate, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::IntoParams))]
#[cfg_attr(feature = "openapi",into_params(parameter_in = Query))]
#[garde(allow_unvalidated)]
pub struct Paging {
    page: Option<u32>,
    #[garde(range(min = 1, max = 1000))]
    page_size: Option<u32>,
    #[garde(length(max = 255))]
    sort: Option<String>,
    #[garde(length(max = 255))]
    filter: Option<String>,
}

impl Paging {
    pub fn into_listing_params(self, default_page_size: u32) -> ApiResult<ListingParams> {
        let page = self.page.unwrap_or(1);
        let page_size = self.page_size.unwrap_or(default_page_size);
        let offset = (page - 1) * page_size;
        let limit = page_size;
        let order = self
            .sort
            .map(|orderings| {
                orderings
                    .split(',')
                    .map(|name| {
                        let (field_name, descending) = match name.trim() {
                            "" => {
                                return Err(ApiError::InvalidQuery(
                                    "Empty ordering name".to_string(),
                                ))
                            }
                            name if name.len() > 100 => {
                                return Err(ApiError::InvalidQuery(
                                    "Ordering name too long".to_string(),
                                ))
                            }
                            name if name.starts_with('+') => (&name[1..], false),
                            name if name.starts_with('-') => (&name[1..], true),
                            name => (name, false),
                        };

                        let order = if descending {
                            mbs4_dal::Order::Desc(field_name.to_string())
                        } else {
                            mbs4_dal::Order::Asc(field_name.to_string())
                        };

                        Ok(order)
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;

        Ok(ListingParams {
            offset: offset.into(),
            limit: limit.into(),
            order,
            filter: None,
        })
    }

    pub fn page_size(&self, default_page_size: u32) -> u32 {
        self.page_size.unwrap_or(default_page_size)
    }
}

#[derive(Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Page<T> {
    page: u32,
    page_size: u32,
    total_pages: u32,
    total: u64,
    rows: Vec<T>,
}

impl<T> Page<T>
where
    T: Serialize,
{
    pub fn try_from_batch(
        batch: Batch<T>,
        page_size: u32,
    ) -> Result<Self, std::num::TryFromIntError> {
        Ok(Self {
            page: u32::try_from(batch.offset)? / page_size + 1,
            page_size,
            total_pages: u32::try_from(
                (u64::try_from(batch.total)? + page_size as u64 - 1) / page_size as u64,
            )?,
            total: batch.total,
            rows: batch.rows,
        })
    }

    pub fn from_batch(batch: Batch<T>, page_size: u32) -> Self {
        Self::try_from_batch(batch, page_size).expect("Failed to convert batch to page")
        // As we control the batch, this should never fail
    }
}
