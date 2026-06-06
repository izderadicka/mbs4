use mbs4_dal::source::EbookSource;

/// Source-extension preference for choosing a "best" input to feed
/// `ebook-convert`. Earlier entries win; ports the ranking from mybookshelf2.
const SOURCE_FORMAT_PRIORITY: &[&str] = &[
    "epub", "mobi", "azw3", "azw", "fb2", "lit", "html", "htm", "rtf", "txt", "pdf", "doc", "docx",
];

fn rank(ext: &str) -> usize {
    let ext = ext.to_ascii_lowercase();
    SOURCE_FORMAT_PRIORITY
        .iter()
        .position(|p| *p == ext)
        .unwrap_or(SOURCE_FORMAT_PRIORITY.len())
}

/// Pick the most suitable source to convert from. Sources whose extension
/// appears earlier in `SOURCE_FORMAT_PRIORITY` are preferred; ties are broken
/// by `created` (most recent first). Unknown extensions sink to the bottom
/// but are still eligible. Returns `None` only when `sources` is empty.
pub fn pick_best_source(sources: &[EbookSource]) -> Option<&EbookSource> {
    sources.iter().min_by(|a, b| {
        rank(&a.format_extension)
            .cmp(&rank(&b.format_extension))
            .then_with(|| b.created.cmp(&a.created))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    fn make(id: i64, ext: &str, created: time::PrimitiveDateTime) -> EbookSource {
        EbookSource {
            id,
            location: format!("book.{ext}"),
            format_name: ext.to_string(),
            format_extension: ext.to_string(),
            size: 1,
            quality: None,
            created_by: None,
            created,
        }
    }

    #[test]
    fn prefers_earlier_in_priority() {
        let sources = vec![
            make(1, "pdf", datetime!(2024-01-01 0:00).into()),
            make(2, "epub", datetime!(2024-01-01 0:00).into()),
            make(3, "mobi", datetime!(2024-01-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 2);
    }

    #[test]
    fn tie_breaks_by_most_recent() {
        let sources = vec![
            make(1, "epub", datetime!(2024-01-01 0:00).into()),
            make(2, "epub", datetime!(2024-06-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 2);
    }

    #[test]
    fn unknown_extension_still_eligible() {
        let sources = vec![make(1, "xyz", datetime!(2024-01-01 0:00).into())];
        assert_eq!(pick_best_source(&sources).unwrap().id, 1);
    }

    #[test]
    fn empty_returns_none() {
        assert!(pick_best_source(&[]).is_none());
    }

    #[test]
    fn case_insensitive() {
        let sources = vec![
            make(1, "PDF", datetime!(2024-01-01 0:00).into()),
            make(2, "EPUB", datetime!(2024-01-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 2);
    }
}
