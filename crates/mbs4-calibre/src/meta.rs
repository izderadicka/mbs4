use lazy_static::lazy_static;
use regex::{Regex, RegexBuilder};

#[derive(Debug)]
pub struct EbookMetadata {
    pub title: Option<String>,
    pub authors: Vec<Author>,
    pub genres: Vec<String>,
    pub language: Option<String>,
    pub series: Option<Series>,
    pub cover_file: Option<String>,
    pub comments: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Author {
    pub first_name: Option<String>,
    pub last_name: String,
}

#[derive(Debug)]
pub struct Series {
    pub title: String,
    pub index: i32,
}

fn build_regex(pattern: &str) -> Regex {
    RegexBuilder::new(pattern)
        .multi_line(true) // same as (?m)
        .unicode(true) // same as re.UNICODE
        .build()
        .unwrap()
}

lazy_static! {
    static ref AUTHORS_RE: Regex = build_regex(r"^Author\(s\)\s*:\s*(.+)");
    static ref TITLE_RE: Regex = build_regex(r"^Title\s*:\s*(.+)");
    static ref TAGS_RE: Regex = build_regex(r"^Tags\s*:\s*(.+)");
    static ref SERIES_RE: Regex = build_regex(r"^Series\s*:\s*(.+)");
    static ref LANGUAGES_RE: Regex = build_regex(r"^Languages\s*:\s*(.+)");
    static ref SERIES_INDEX_RE: Regex = Regex::new(r"^(.*) #(\d+)$").unwrap();
    static ref COMMENTS_RE: Regex = build_regex(r"^Comments\s*:\s*(.+)");
    static ref BRACKETS_RE: Regex = Regex::new(r"\[[^\]]+\]").unwrap();
}

pub fn parse_metadata(data: &str) -> EbookMetadata {
    let mut title = None;
    let mut authors = Vec::new();
    let mut genres = Vec::new();
    let mut language = None;
    let mut series = None;
    let mut comments = None;

    // Title
    if let Some(caps) = TITLE_RE.captures(data) {
        title = Some(caps[1].trim().to_string());
    }

    // Authors
    if let Some(caps) = AUTHORS_RE.captures(data) {
        let mut authors_str = caps[1].to_string();
        authors_str = BRACKETS_RE.replace_all(&authors_str, "").to_string();
        authors = authors_str
            .split('&')
            .filter_map(|a| {
                let trimmed = a.trim();
                if !trimmed.is_empty() {
                    parse_author(trimmed)
                } else {
                    None
                }
            })
            .collect();
    }

    // Tags (Genres)
    if let Some(caps) = TAGS_RE.captures(data) {
        genres = caps[1]
            .split(',')
            .map(|g| g.trim().to_string())
            .filter(|g| !g.is_empty())
            .collect();
    }

    // Language
    if let Some(caps) = LANGUAGES_RE.captures(data) {
        language = caps[1].split(',').next().map(|l| l.trim().to_string());
    }

    // Series
    if let Some(caps) = SERIES_RE.captures(data) {
        let series_str = caps[1].trim();
        if let Some(series_caps) = SERIES_INDEX_RE.captures(series_str) {
            series = Some(Series {
                title: series_caps[1].trim().to_string(),
                index: series_caps[2].parse::<i32>().unwrap_or(0),
            });
        }
    }

    // Comments
    if let Some(caps) = COMMENTS_RE.captures(data) {
        let comments_str = caps[1].trim().to_string();
        if comments_str.len() > 3 {
            comments = Some(comments_str);
        }
    }

    EbookMetadata {
        title,
        authors,
        genres,
        language,
        series,
        cover_file: None,
        comments,
    }
}

fn parse_author(author: &str) -> Option<Author> {
    let parts: Vec<&str> = author.split(',').map(|s| s.trim()).collect();

    if parts.len() > 1 {
        Some(Author {
            last_name: parts[0].to_string(),
            first_name: Some(parts[1..].join(" ")),
        })
    } else {
        let words: Vec<&str> = author.split_whitespace().collect();
        if !words.is_empty() {
            let last_name = words.last().unwrap().to_string();
            let first_name = if words.len() > 1 {
                Some(words[..words.len() - 1].join(" "))
            } else {
                None
            };
            Some(Author {
                last_name,
                first_name,
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_author() {
        assert_eq!(
            parse_author("John Doe"),
            Some(Author {
                last_name: "Doe".to_string(),
                first_name: Some("John".to_string())
            })
        );
        assert_eq!(
            parse_author("John"),
            Some(Author {
                last_name: "John".to_string(),
                first_name: None
            })
        );
        assert_eq!(
            parse_author("John Doe Smith"),
            Some(Author {
                last_name: "Smith".to_string(),
                first_name: Some("John Doe".to_string())
            })
        );
        assert_eq!(
            parse_author("Smith, John Doe"),
            Some(Author {
                last_name: "Smith".to_string(),
                first_name: Some("John Doe".to_string())
            })
        );
    }

    #[test]
    fn test_parse_metadata() {
        let data = "Title               : Armagedony
Title sort          : Armagedony
Author(s)           : Jack Dann & Gardner Raymond Dozois & Frederik Pohl & Gregory Benford & Nancy Kressová & Richard Cowper & Howard Waldrop & Racoona Sheldon & Fritz Leiber & Allan Danzig & Larry Niven & Geoffrey A. Landis & William Barton [Dann, Jack & Dozois, Gardner Raymond & Pohl, Frederik & Benford, Gregory & Kressová, Nancy & Cowper, Richard & Waldrop, Howard & Sheldon, Racoona & Leiber, Fritz & Danzig, Allan & Niven, Larry & Landis, Geoffrey A. & Barton, William]
Publisher           : Triton
Tags                : Sci-fi, povídky
Languages           : ces
Published           : 2005-06-14T22:00:00+00:00
Identifiers         : isbn:80-7254-646-5
Comments            : Sbírka dvanácti apokalyptických scénářů, v nichž celosvětově známí autoři fantastiky jako Gregory Benford, Gardner Dozois, Nancy Kress, Geoffrey A. Landis, Fritz Leiber, Larry Niven, Frederik Pohl či James Tiptree, Jr. nabízejí hrůzu nahánějící vize konce času, ať už ho způsobili bohové, technologie či sama příroda.";

        let metadata = parse_metadata(data);
        assert_eq!(metadata.title.unwrap(), "Armagedony");
        assert_eq!(metadata.authors.len(), 13);
        assert_eq!(metadata.genres.len(), 2);
        assert_eq!(metadata.language, Some("ces".to_string()));
        assert!(metadata.series.is_none());
        assert!(metadata.comments.unwrap().len() > 80);
    }
}
