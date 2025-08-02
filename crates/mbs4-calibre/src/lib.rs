use std::process::Stdio;

use crate::meta::{EbookMetadata, parse_metadata};

pub mod meta;

const EBOOK_META_PROGRAM: &str = "ebook-meta";

pub async fn extract_metadata(path: &str) -> anyhow::Result<EbookMetadata> {
    let output = tokio::process::Command::new(EBOOK_META_PROGRAM)
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .await?;

    if output.status.success() {
        let stdout = std::str::from_utf8(&output.stdout)?;
        let meta = parse_metadata(stdout);
        Ok(meta)
    } else {
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        Err(anyhow::anyhow!(
            "ebook-meta failed with status: {}",
            output.status
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_extract_metadata() {
        print!("Current dir: {:#?}\n", std::env::current_dir());
        let metadata = extract_metadata("../../test-data/samples/Holmes.epub")
            .await
            .unwrap();
        assert_eq!(metadata.title.unwrap(), "The Adventures of Sherlock Holmes");
        assert_eq!(metadata.authors.len(), 1);
        assert_eq!(metadata.authors[0].last_name, "Doyle");
        assert_eq!(
            metadata.authors[0].first_name.as_ref().unwrap(),
            "Arthur Conan"
        );
        assert_eq!(metadata.genres.len(), 5);
        assert_eq!(metadata.language, Some("eng".to_string()));
        assert!(metadata.series.is_none());
    }
}
