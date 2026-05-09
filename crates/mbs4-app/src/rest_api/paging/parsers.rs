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
        .map_err(|e| ApiError::InvalidQuery(format!("Invalid filter error {e}")))
}

#[cfg(test)]
mod tests {
    use mbs4_dal::Order;

    use super::*;

    fn order_field(o: &Order) -> &str {
        match o {
            Order::Asc(s) | Order::Desc(s) => s.as_str(),
        }
    }

    fn is_desc(o: &Order) -> bool {
        matches!(o, Order::Desc(_))
    }

    #[test]
    fn test_parse_ordering_ascending() {
        let result = parse_ordering("+title".to_string()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(order_field(&result[0]), "title");
        assert!(!is_desc(&result[0]));
    }

    #[test]
    fn test_parse_ordering_descending() {
        let result = parse_ordering("-title".to_string()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(order_field(&result[0]), "title");
        assert!(is_desc(&result[0]));
    }

    #[test]
    fn test_parse_ordering_default_is_ascending() {
        let result = parse_ordering("title".to_string()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(order_field(&result[0]), "title");
        assert!(!is_desc(&result[0]));
    }

    #[test]
    fn test_parse_ordering_multiple_fields() {
        let result = parse_ordering("+title,-author".to_string()).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(order_field(&result[0]), "title");
        assert!(!is_desc(&result[0]));
        assert_eq!(order_field(&result[1]), "author");
        assert!(is_desc(&result[1]));
    }

    #[test]
    fn test_parse_ordering_empty_name_is_error() {
        assert!(parse_ordering("".to_string()).is_err());
        assert!(parse_ordering(",title".to_string()).is_err()); // leading comma → empty segment
    }

    #[test]
    fn test_parse_ordering_name_too_long_is_error() {
        let long = "a".repeat(101);
        assert!(parse_ordering(long).is_err());
    }

    #[test]
    fn test_parse_ordering_exactly_100_chars_is_ok() {
        let ok = "a".repeat(100);
        assert!(parse_ordering(ok).is_ok());
    }

    #[test]
    fn test_parse_filters_too_long_is_error() {
        let long = "a".repeat(1001);
        assert!(parse_filters(long).is_err());
    }

    #[test]
    fn test_parse_filters_length_boundary() {
        // Build a string of exactly 1000 chars and confirm the length guard does not fire.
        let at_limit: String = "a".repeat(1000);
        // This won't be a valid filter but shouldn't error with "too long".
        if let Err(e) = parse_filters(at_limit) {
            assert!(
                !format!("{e:?}").contains("too long"),
                "length guard fired at exactly 1000"
            );
        }
    }
}
