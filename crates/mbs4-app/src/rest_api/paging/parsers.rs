use mbs4_dal::{Filter, Order};

use crate::error::{ApiError, ApiResult};

pub(super) fn parse_ordering(orderings: String) -> ApiResult<Vec<Order>> {
    orderings
        .split(',')
        .map(|name| {
            let (field_name, descending) = match name.trim() {
                "" => return Err(ApiError::InvalidQuery("Empty ordering name".to_string())),
                name if name.len() > 100 => {
                    return Err(ApiError::InvalidQuery("Ordering name too long".to_string()))
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
}

pub(super) fn parse_filters(filters: String) -> ApiResult<Vec<Filter>> {
    if filters.len() > 1000 {
        return Err(ApiError::InvalidQuery("Filter too long".to_string()));
    }

    filters
        .split(';')
        .map(|s| s.parse())
        .collect::<Result<Vec<Filter>, _>>()
        .or_else(|e| Err(ApiError::InvalidQuery(format!("Invalid filter error {e}"))))
}
