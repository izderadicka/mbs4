use mbs4_dal::source::EbookSource;

const SOURCE_FORMAT_PRIORITY: &[&str] = &[
    // Quality-aware prefix.
    "epub", "mobi", "docx", "doc", //
    // Remaining whitelist, ordered by suitability as a conversion source.
    "odt", "html", "htm", "rtf", "azw3", //
    // not so good, but still better than unknown formats.
    "azw", "pdb", "fb2", "lit", "prc", "chm", "txt", "pdf",
];
const QUALITY_AWARE_PREFIX_LEN: u8 = 4;

fn priority_rank(source: &EbookSource) -> Option<(u8, u8)> {
    let ext = source.format_extension.to_ascii_lowercase();
    let pos = SOURCE_FORMAT_PRIORITY.iter().position(|p| *p == ext)? as u8; // it's ok will never have more than 255 formats in the whitelist
    let quality_penalty = if pos < QUALITY_AWARE_PREFIX_LEN {
        quality_penalty(source.quality)
    } else {
        100
    };
    Some((quality_penalty, pos))
}

fn quality_penalty(q: Option<f32>) -> u8 {
    // kinda based on existing values in the db
    match q {
        None => 1,
        Some(q) if q <= 30.0 => 2,
        Some(q) if q > 60.0 => 0,
        Some(_) => 1,
    }
}

pub fn pick_best_source(sources: &[EbookSource]) -> Option<&EbookSource> {
    sources
        .iter()
        .filter(|s| priority_rank(s).is_some())
        .min_by(|a, b| {
            let a_rank = priority_rank(a).unwrap(); // safe because of the filter above
            let b_rank = priority_rank(b).unwrap();
            a_rank.cmp(&b_rank).then_with(|| b.created.cmp(&a.created)) // more recent wins ties
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
        let sources = vec![
            make_q(1, "epub", None, datetime!(2024-01-01 0:00).into()),
            make_q(2, "pdf", Some(99.0), datetime!(2024-06-01 0:00).into()),
            make(3, "azw3", datetime!(2024-06-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 1);
    }

    #[test]
    fn none_beats_low_bucket_within_prefix() {
        let sources = vec![
            make_q(1, "epub", None, datetime!(2024-01-01 0:00).into()),
            make_q(2, "mobi", Some(10.0), datetime!(2024-06-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 1);
    }

    #[test]
    fn none_loses_to_high_bucket_within_prefix() {
        let sources = vec![
            make_q(1, "epub", None, datetime!(2024-01-01 0:00).into()),
            make_q(2, "mobi", Some(90.0), datetime!(2024-06-01 0:00).into()),
        ];
        assert_eq!(pick_best_source(&sources).unwrap().id, 2);
    }

    #[test]
    fn same_bucket_lower_rank_wins_within_prefix() {
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
    fn bucket_boundary_at_30() {
        // q=19 → bucket 0, q=20 → bucket 1.
        let sources = vec![
            make_q(1, "epub", Some(19.0), datetime!(2024-06-01 0:00).into()),
            make_q(2, "epub", Some(31.0), datetime!(2024-01-01 0:00).into()),
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
