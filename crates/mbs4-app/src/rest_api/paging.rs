use crate::error::ApiResult;
use garde::Validate;
use mbs4_dal::{Batch, ListingParams};
use serde::Serialize;

mod parsers;

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
        let order = self.sort.map(parsers::parse_ordering).transpose()?;

        let filter = self.filter.map(parsers::parse_filters).transpose()?;

        Ok(ListingParams {
            offset: offset.into(),
            limit: limit.into(),
            order,
            filter,
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
            total_pages: u32::try_from(batch.total.div_ceil(page_size as u64))?,
            total: batch.total,
            rows: batch.rows,
        })
    }

    pub fn from_batch(batch: Batch<T>, page_size: u32) -> Self {
        Self::try_from_batch(batch, page_size).expect("Failed to convert batch to page")
        // As we control the batch, this should never fail
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paging(page: u32, page_size: u32) -> Paging {
        Paging {
            page: Some(page),
            page_size: Some(page_size),
            sort: None,
            filter: None,
        }
    }

    fn batch(offset: i64, total: u64) -> Batch<()> {
        Batch {
            offset,
            limit: 0,
            total,
            rows: vec![],
        }
    }

    #[test]
    fn test_page1_offset_is_zero() {
        let p = paging(1, 20).into_listing_params(20).unwrap();
        assert_eq!(p.offset, 0);
        assert_eq!(p.limit, 20);
    }

    #[test]
    fn test_page2_offset_equals_page_size() {
        let p = paging(2, 20).into_listing_params(20).unwrap();
        assert_eq!(p.offset, 20);
        assert_eq!(p.limit, 20);
    }

    #[test]
    fn test_page3_with_small_page_size() {
        let p = paging(3, 10).into_listing_params(10).unwrap();
        assert_eq!(p.offset, 20);
        assert_eq!(p.limit, 10);
    }

    #[test]
    fn test_default_page_size_used_when_unset() {
        let p = Paging {
            page: None,
            page_size: None,
            sort: None,
            filter: None,
        }
        .into_listing_params(15)
        .unwrap();
        assert_eq!(p.offset, 0);
        assert_eq!(p.limit, 15);
    }

    #[test]
    fn test_page_number_from_batch_offset() {
        assert_eq!(Page::<()>::from_batch(batch(0, 50), 20).page, 1);
        assert_eq!(Page::<()>::from_batch(batch(20, 50), 20).page, 2);
        assert_eq!(Page::<()>::from_batch(batch(40, 50), 20).page, 3);
    }

    #[test]
    fn test_total_pages_div_ceil() {
        assert_eq!(Page::<()>::from_batch(batch(0, 50), 20).total_pages, 3);
        assert_eq!(Page::<()>::from_batch(batch(0, 40), 20).total_pages, 2); // exact fit
        assert_eq!(Page::<()>::from_batch(batch(0, 41), 20).total_pages, 3); // one item over
        assert_eq!(Page::<()>::from_batch(batch(0, 1), 20).total_pages, 1);
        assert_eq!(Page::<()>::from_batch(batch(0, 0), 20).total_pages, 0);
    }
}
