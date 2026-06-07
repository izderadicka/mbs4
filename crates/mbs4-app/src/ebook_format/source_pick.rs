use mbs4_dal::source::EbookSource;

/// Ordered preference list for choosing a source to feed `ebook-convert`.
///
/// - Indexes `< QUALITY_AWARE_PREFIX_LEN` are **quality-aware**: within the
///   prefix, the bucketed `quality` score dominates the rank-based ordering.
/// - Anything **not in this list** is ineligible (replaces the old
///   blocklist — `djvu`, `cbz`, `cbr`, `cb7`, etc. are simply absent).
///
/// To add a convertible format, insert it at the desired position. To mark a
/// format as quality-aware, place it above index `QUALITY_AWARE_PREFIX_LEN`
/// (and bump the constant).
const SOURCE_FORMAT_PRIORITY: &[&str] = &[
    // Quality-aware prefix.
    "epub", "mobi", "doc", "docx", //
    // Remaining whitelist, ordered by suitability as a conversion source.
    "odt", "azw3", "azw", "fb2", "lit", "prc", "chm", //
    "html", "htm", "rtf", "txt", "pdf",
];
const QUALITY_AWARE_PREFIX_LEN: usize = 4;

fn priority_rank(ext: &str) -> Option<usize> {
    let ext = ext.to_ascii_lowercase();
    SOURCE_FORMAT_PRIORITY.iter().position(|p| *p == ext)
}

fn is_quality_aware(rank: usize) -> bool {
    rank < QUALITY_AWARE_PREFIX_LEN
}

/// 0..=4 for valid 0–100 quality, 2 for None ("unrated = assumed average").
fn quality_bucket(q: Option<f32>) -> i32 {
    match q {
        None => 2,
        Some(q) => ((q.clamp(0.0, 100.0) / 20.0).floor() as i32).min(4),
    }
}

/// Pick the most suitable source to convert from.
///
/// Selection rules (smaller "key" wins):
/// 1. Sources whose extension is not in `SOURCE_FORMAT_PRIORITY` are filtered out.
/// 2. A source whose rank is inside the quality-aware prefix always beats a
///    source outside the prefix.
/// 3. Within the prefix: higher `quality_bucket` wins (None ranks as bucket 2);
///    same bucket → lower rank wins; same rank → more recent `created` wins.
/// 4. Outside the prefix: lower rank wins; same rank → more recent `created` wins.
///
/// Returns `None` if the input is empty or no source has a whitelisted extension.
pub fn pick_best_source(sources: &[EbookSource]) -> Option<&EbookSource> {
    sources
        .iter()
        .filter(|s| priority_rank(&s.format_extension).is_some())
        .min_by(|a, b| {
            let a_rank = priority_rank(&a.format_extension).unwrap();
            let b_rank = priority_rank(&b.format_extension).unwrap();
            let a_aware = is_quality_aware(a_rank);
            let b_aware = is_quality_aware(b_rank);

            match (a_aware, b_aware) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                (true, true) => quality_bucket(b.quality)
                    .cmp(&quality_bucket(a.quality))
                    .then_with(|| a_rank.cmp(&b_rank))
                    .then_with(|| b.created.cmp(&a.created)),
                (false, false) => a_rank.cmp(&b_rank).then_with(|| b.created.cmp(&a.created)),
            }
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
    fn tie_breaks_by_most_recent_outside_prefix() {
        let sources = vec![
            make(1, "pdf", datetime!(2024-01-01 0:00).into()),
            make(2, "pdf", datetime!(2024-06-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 2);
    }

    #[test]
    fn unknown_extension_filtered() {
        let sources = vec![make(1, "xyz", datetime!(2024-01-01 0:00).into())];
        assert!(pick_best_source(&sources).is_none());
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
    fn non_whitelisted_filtered() {
        let sources = vec![
            make(1, "djvu", datetime!(2024-06-01 0:00).into()),
            make(2, "pdf", datetime!(2024-01-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 2);
    }

    #[test]
    fn only_non_whitelisted_returns_none() {
        let sources = vec![
            make(1, "djvu", datetime!(2024-01-01 0:00).into()),
            make(2, "cbz", datetime!(2024-06-01 0:00).into()),
        ];
        assert!(pick_best_source(&sources).is_none());
    }

    #[test]
    fn higher_bucket_wins_within_prefix() {
        // epub @ 90 (bucket 4) beats epub @ 40 (bucket 2), regardless of recency.
        let sources = vec![
            make_q(1, "epub", Some(40.0), datetime!(2024-06-01 0:00).into()),
            make_q(2, "epub", Some(90.0), datetime!(2024-01-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 2);
    }

    #[test]
    fn same_bucket_recency_decides_within_prefix() {
        // Both None → bucket 2; same format → recency wins.
        let sources = vec![
            make_q(1, "mobi", None, datetime!(2024-06-01 0:00).into()),
            make_q(2, "mobi", Some(50.0), datetime!(2024-01-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 1);
    }

    #[test]
    fn quality_ignored_outside_prefix() {
        // pdf is not in the prefix; quality is irrelevant, recency decides.
        let sources = vec![
            make_q(1, "pdf", Some(90.0), datetime!(2024-01-01 0:00).into()),
            make_q(2, "pdf", Some(10.0), datetime!(2024-06-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 2);
    }

    #[test]
    fn higher_bucket_beats_format_rank_within_prefix() {
        // mobi bucket 4 beats epub bucket 0 even though epub ranks higher.
        let sources = vec![
            make_q(1, "epub", Some(10.0), datetime!(2024-01-01 0:00).into()),
            make_q(2, "mobi", Some(99.0), datetime!(2024-06-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 2);
    }

    #[test]
    fn prefix_beats_non_prefix_even_with_none() {
        // epub @ None (Tier A, bucket 2) beats any non-prefix source.
        let sources = vec![
            make_q(1, "epub", None, datetime!(2024-01-01 0:00).into()),
            make_q(2, "pdf", Some(99.0), datetime!(2024-06-01 0:00).into()),
            make(3, "azw3", datetime!(2024-06-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 1);
    }

    #[test]
    fn none_beats_low_bucket_within_prefix() {
        // epub @ None (bucket 2) vs mobi @ 10 (bucket 0). Higher bucket wins.
        let sources = vec![
            make_q(1, "epub", None, datetime!(2024-01-01 0:00).into()),
            make_q(2, "mobi", Some(10.0), datetime!(2024-06-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 1);
    }

    #[test]
    fn none_loses_to_high_bucket_within_prefix() {
        // epub @ None (bucket 2) vs mobi @ 90 (bucket 4). Higher bucket wins.
        let sources = vec![
            make_q(1, "epub", None, datetime!(2024-01-01 0:00).into()),
            make_q(2, "mobi", Some(90.0), datetime!(2024-06-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 2);
    }

    #[test]
    fn same_bucket_lower_rank_wins_within_prefix() {
        // Both None → bucket 2; lower rank (epub) beats mobi/doc/docx.
        let sources = vec![
            make_q(1, "mobi", None, datetime!(2024-06-01 0:00).into()),
            make_q(2, "epub", None, datetime!(2024-01-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 2);
    }

    #[test]
    fn non_prefix_format_rank() {
        // fb2 outranks pdf inside the non-prefix tier.
        let sources = vec![
            make(1, "pdf", datetime!(2024-06-01 0:00).into()),
            make(2, "fb2", datetime!(2024-01-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 2);
    }

    #[test]
    fn bucket_boundary_at_20() {
        // q=19 → bucket 0, q=20 → bucket 1.
        let sources = vec![
            make_q(1, "epub", Some(19.0), datetime!(2024-06-01 0:00).into()),
            make_q(2, "epub", Some(20.0), datetime!(2024-01-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 2);
    }

    #[test]
    fn top_bucket_clamps_100() {
        // q=81 and q=100 both land in bucket 4; recency decides.
        let sources = vec![
            make_q(1, "epub", Some(81.0), datetime!(2024-06-01 0:00).into()),
            make_q(2, "epub", Some(100.0), datetime!(2024-01-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 1);
    }
}
