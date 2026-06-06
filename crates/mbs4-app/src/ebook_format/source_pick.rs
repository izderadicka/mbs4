use std::cmp::Ordering;

use mbs4_dal::source::EbookSource;

/// Source-extension preference for choosing a "best" input to feed
/// `ebook-convert`. Earlier entries win.
const SOURCE_FORMAT_PRIORITY: &[&str] = &[
    "epub", "mobi", "azw3", "azw", "fb2", "lit", "html", "htm", "rtf", "txt", "pdf", "doc", "docx",
];

/// Formats `ebook-convert` cannot meaningfully process as a source.
/// Sources of these formats are filtered out entirely.
const SOURCE_FORMAT_BLOCKLIST: &[&str] = &["djvu", "cbz", "cbr", "cb7"];

/// Formats where the `quality` column is informative — within the same
/// extension, prefer the source with higher quality. (`None` ranks below any
/// `Some(_)`.)
const QUALITY_AWARE_FORMATS: &[&str] = &["epub", "mobi", "doc", "docx"];

fn ext_lower(s: &EbookSource) -> String {
    s.format_extension.to_ascii_lowercase()
}

fn is_blocklisted(ext: &str) -> bool {
    SOURCE_FORMAT_BLOCKLIST.iter().any(|p| *p == ext)
}

fn priority_rank(ext: &str) -> usize {
    SOURCE_FORMAT_PRIORITY
        .iter()
        .position(|p| *p == ext)
        .unwrap_or(SOURCE_FORMAT_PRIORITY.len())
}

fn is_quality_aware(ext: &str) -> bool {
    QUALITY_AWARE_FORMATS.iter().any(|p| *p == ext)
}

/// Pick the most suitable source to convert from.
///
/// Selection rules, applied in order:
/// 1. Sources whose extension is in `SOURCE_FORMAT_BLOCKLIST` (e.g. `djvu`,
///    `cbz`) are removed — they cannot be converted.
/// 2. Lower index in `SOURCE_FORMAT_PRIORITY` wins. Unknown extensions sink to
///    the bottom but are still eligible.
/// 3. Within the same extension, if it is one of `QUALITY_AWARE_FORMATS`
///    (`epub`, `mobi`, `doc`, `docx`), higher `quality` wins (`None` is treated
///    as lowest).
/// 4. Most recent `created` wins.
///
/// Returns `None` only when no source is eligible (empty input or every
/// source was blocklisted).
pub fn pick_best_source(sources: &[EbookSource]) -> Option<&EbookSource> {
    sources
        .iter()
        .filter(|s| !is_blocklisted(&ext_lower(s)))
        .min_by(|a, b| {
            let a_ext = ext_lower(a);
            let b_ext = ext_lower(b);
            priority_rank(&a_ext)
                .cmp(&priority_rank(&b_ext))
                .then_with(|| {
                    if a_ext == b_ext && is_quality_aware(&a_ext) {
                        b.quality.partial_cmp(&a.quality).unwrap_or(Ordering::Equal)
                    } else {
                        Ordering::Equal
                    }
                })
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

    fn make_q(
        id: i64,
        ext: &str,
        quality: Option<f32>,
        created: time::PrimitiveDateTime,
    ) -> EbookSource {
        let mut s = make(id, ext, created);
        s.quality = quality;
        s
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
            make(1, "pdf", datetime!(2024-01-01 0:00).into()),
            make(2, "pdf", datetime!(2024-06-01 0:00).into()),
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

    #[test]
    fn blocklisted_formats_filtered() {
        let sources = vec![
            make(1, "djvu", datetime!(2024-06-01 0:00).into()),
            make(2, "pdf", datetime!(2024-01-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 2);
    }

    #[test]
    fn only_blocklisted_returns_none() {
        let sources = vec![
            make(1, "djvu", datetime!(2024-01-01 0:00).into()),
            make(2, "cbz", datetime!(2024-06-01 0:00).into()),
        ];
        assert!(pick_best_source(&sources).is_none());
    }

    #[test]
    fn higher_quality_wins_for_epub() {
        let sources = vec![
            make_q(1, "epub", Some(40.0), datetime!(2024-06-01 0:00).into()),
            make_q(2, "epub", Some(90.0), datetime!(2024-01-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 2);
    }

    #[test]
    fn quality_some_beats_quality_none_for_mobi() {
        let sources = vec![
            make_q(1, "mobi", None, datetime!(2024-06-01 0:00).into()),
            make_q(2, "mobi", Some(50.0), datetime!(2024-01-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 2);
    }

    #[test]
    fn quality_ignored_for_non_aware_formats() {
        // pdf is NOT quality-aware; tie-break falls through to most recent.
        let sources = vec![
            make_q(1, "pdf", Some(90.0), datetime!(2024-01-01 0:00).into()),
            make_q(2, "pdf", Some(10.0), datetime!(2024-06-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 2);
    }

    #[test]
    fn format_priority_beats_quality() {
        // epub with low quality still beats higher-priority-rank mobi.
        let sources = vec![
            make_q(1, "epub", Some(10.0), datetime!(2024-01-01 0:00).into()),
            make_q(2, "mobi", Some(99.0), datetime!(2024-06-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 1);
    }
}
