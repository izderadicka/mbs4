use phf::phf_map;
use std::path::Path;
use unicode_normalization::UnicodeNormalization;

fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|n| n.chars().next())
        .map(|c| c.to_uppercase().collect::<String>())
        .collect::<Vec<String>>()
        .join(" ")
}

#[derive(Debug)]
pub struct Author<'a> {
    pub first_name: Option<&'a str>,
    pub last_name: &'a str,
}

#[derive(Debug)]
pub struct Ebook<'a> {
    pub title: &'a str,
    pub authors: Vec<Author<'a>>,
    pub language_code: &'a str,
    pub series_name: Option<&'a str>,
    pub series_index: Option<u32>,
}

impl<'a> Ebook<'a> {
    fn authors_str(&self) -> String {
        match self.authors.len() {
            0 => "No Authors".to_string(),
            1 => {
                let a = &self.authors[0];
                if let Some(first_name) = a.first_name {
                    format!("{} {}", a.last_name, first_name)
                } else {
                    a.last_name.to_string()
                }
            }
            _ => {
                let mut authors = vec![];
                for a in self.authors.iter().take(3) {
                    let s = if let Some(first_name) = a.first_name {
                        format!("{} {}", a.last_name, initials(first_name))
                    } else {
                        a.last_name.to_string()
                    };
                    authors.push(s);
                }
                let mut s = authors.join(", ");
                if self.authors.len() > 3 {
                    s.push_str(" and others");
                }
                s
            }
        }
    }

    fn norm_file_name_base(&self) -> String {
        let author = safe_file_name(&self.authors_str());
        let title = safe_file_name(self.title);
        let language = self.language_code;

        let name = if let Some(series) = self.series_name {
            let serie = safe_file_name(series);
            let serie_index = self.series_index.unwrap_or(0);
            format!(
                "{}/{}/{} {} - {}({})/{} - {} {} - {}",
                author,
                serie,
                serie,
                serie_index,
                title,
                language,
                author,
                serie,
                serie_index,
                title
            )
        } else {
            format!("{}/{}({})/{} - {}", author, title, language, author, title)
        };

        let sanitized = remove_diacritics(&name);
        assert!(sanitized.len() < 4096);
        sanitized
    }

    pub fn norm_file_name(&self, ext: &str) -> String {
        let mut name = self.norm_file_name_base();
        for ch in [':', '*', '%', '|', '"', '<', '>', '?', '\\'] {
            name = name.replace(ch, "");
        }
        if !ext.is_empty() {
            name.push('.');
            name.push_str(ext);
        }
        name
    }

    pub fn norm_file_name_only(&self, ext: &str) -> String {
        let full = self.norm_file_name(ext);
        Path::new(&full)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or(full)
    }

    pub fn ebook_base_dir(&self) -> Option<String> {
        Path::new(&self.norm_file_name(""))
            .parent()
            .map(|p| p.to_string_lossy().to_string())
    }
}

static ND_CHARMAP: phf::Map<char, &'static str> = phf_map! {
    'Æ' => "AE",
    'æ' => "ae",
    'Ð' => "D",
    'ð' => "d",
    'Ø' => "O",
    'ø' => "o",
    'Þ' => "Th",
    'þ' => "th",
    'ß' => "s",
    'Đ' => "D",
    'đ' => "d",
    'Ħ' => "H",
    'ħ' => "h",
    'ı' => "i",
    'ĸ' => "k",
    'Ł' => "L",
    'ł' => "l",
    'Ŋ' => "N",
    'ŋ' => "n",
    'Œ' => "Oe",
    'œ' => "oe",
    'Ŧ' => "T",
    'ŧ' => "t",
};

fn remove_diacritics(text: &str) -> String {
    let mut result = String::with_capacity(text.len());

    for c in text.nfkd() {
        if let Some(mapped) = ND_CHARMAP.get(&c) {
            result.push_str(mapped); // efficient: appends characters directly
        } else if c.is_ascii() {
            result.push(c);
        } else if c.is_alphabetic() {
            result.push(' ');
        }
    }

    result
}

fn safe_file_name(name: &str) -> String {
    name.replace("/", "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_author(last: &'static str, first: Option<&'static str>) -> Author<'static> {
        Author {
            last_name: last,
            first_name: first,
        }
    }

    fn ebook_with_authors(authors: Vec<Author<'_>>) -> Ebook<'_> {
        Ebook {
            title: "T",
            authors,
            language_code: "en",
            series_name: None,
            series_index: None,
        }
    }

    #[test]
    fn test_authors_str_no_authors() {
        assert_eq!(ebook_with_authors(vec![]).authors_str(), "No Authors");
    }

    #[test]
    fn test_authors_str_single_with_first_name() {
        let book = ebook_with_authors(vec![mk_author("Doyle", Some("Arthur"))]);
        assert_eq!(book.authors_str(), "Doyle Arthur");
    }

    #[test]
    fn test_authors_str_single_without_first_name() {
        let book = ebook_with_authors(vec![mk_author("Voltaire", None)]);
        assert_eq!(book.authors_str(), "Voltaire");
    }

    #[test]
    fn test_authors_str_three_authors_no_truncation() {
        let book = ebook_with_authors(vec![
            mk_author("Smith", Some("Alice")),
            mk_author("Jones", Some("Bob")),
            mk_author("Brown", Some("Carol")),
        ]);
        let s = book.authors_str();
        assert!(
            !s.contains("and others"),
            "3 authors should not get 'and others': {s}"
        );
        assert!(
            s.contains("Smith") && s.contains("Jones") && s.contains("Brown"),
            "{s}"
        );
    }

    #[test]
    fn test_authors_str_four_authors_truncated() {
        let book = ebook_with_authors(vec![
            mk_author("Smith", Some("Alice")),
            mk_author("Jones", Some("Bob")),
            mk_author("Brown", Some("Carol")),
            mk_author("White", Some("Dave")),
        ]);
        let s = book.authors_str();
        assert!(
            s.contains("and others"),
            "4 authors should append 'and others': {s}"
        );
        assert!(!s.contains("White"), "4th author should not appear: {s}");
    }

    #[test]
    fn test_remove_diacritics() {
        assert_eq!(remove_diacritics("Æ"), "AE");
        assert_eq!(remove_diacritics("æ"), "ae");
        assert_eq!(remove_diacritics("Œ"), "Oe");
        assert_eq!(remove_diacritics("œ"), "oe");
    }

    #[test]
    fn test_naming() {
        let authors = vec![
            Author {
                first_name: Some("Jan"),
                last_name: "Příšerně",
            },
            Author {
                first_name: Some("Zdeněk"),
                last_name: "Žluťoučký",
            },
        ];
        let book = Ebook {
            title: "Pěl ďábelské",
            authors,
            language_code: "cs",
            series_name: Some("ódy"),
            series_index: Some(1),
        };

        let dirname = book.ebook_base_dir().unwrap();
        assert_eq!(
            "Priserne J, Zlutoucky Z/ody/ody 1 - Pel dabelske(cs)",
            dirname
        );
        let filename = book.norm_file_name("epub");
        assert_eq!(
            "Priserne J, Zlutoucky Z/ody/ody 1 - Pel dabelske(cs)/Priserne J, Zlutoucky Z - ody 1 - Pel dabelske.epub",
            filename
        );
    }
}
