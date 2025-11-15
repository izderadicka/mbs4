use mbs4_dal::{ebook::EbookRepository, ListingParams, Order};

use crate::search::Search;

pub enum DependentId {
    Author(i64),
    Series(i64),
}

pub async fn reindex_books(
    repository: &EbookRepository,
    indexer: &Search,
    id: DependentId,
) -> anyhow::Result<()> {
    let mut sent = 0;
    loop {
        let params = ListingParams {
            limit: 100,
            offset: sent,
            order: Some(vec![Order::Asc("e.id".into())]),
            filter: None,
        };
        let res = match id {
            DependentId::Author(author_id) => repository.list_by_author(params, author_id).await?,
            DependentId::Series(series_id) => repository.list_by_series(params, series_id).await?,
        };
        let books = repository.map_short_to_ebooks(&res.rows).await?;
        indexer.index_books(books, true)?;
        let read: i64 = res.rows.len().try_into().unwrap(); // this cannot practically happen, but better to be safe then sorry and using unsafe conversions
        let total: i64 = res.total.try_into().unwrap();
        sent += read;
        if sent >= total {
            break;
        }
    }

    Ok(())
}
