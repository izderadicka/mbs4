use std::process::Stdio;

use crate::meta::{EbookMetadata, parse_metadata};

pub mod meta;

const EBOOK_META_PROGRAM: &str = "ebook-meta";

pub async fn extract_metadata(path: &str, extract_cover: bool) -> anyhow::Result<EbookMetadata> {
    let mut cmd = tokio::process::Command::new(EBOOK_META_PROGRAM);
    let mut cover_file = None;

    cmd.arg(path).stdin(Stdio::null());

    if extract_cover {
        let tmp_name =
            std::env::temp_dir().join(format!("mbs4-cover-{}.jpg", uuid::Uuid::new_v4()));
        cmd.arg("--get-cover");
        cmd.arg(&tmp_name);
        cover_file = Some(tmp_name);
    }

    let output = cmd.output().await?;

    if output.status.success() {
        let stdout = std::str::from_utf8(&output.stdout)?;
        let mut meta = parse_metadata(stdout);
        if let Some(cover_file) = cover_file {
            if tokio::fs::metadata(&cover_file).await.is_ok() {
                meta.cover_file = Some(cover_file.to_string_lossy().into());
            }
        }
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
        let metadata = extract_metadata("../../test-data/samples/Holmes.epub", true)
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

        let cover_file = metadata.cover_file.unwrap();
        let file_meta = tokio::fs::metadata(&cover_file).await.unwrap();
        assert!(file_meta.is_file());
        assert!(file_meta.len() > 100_000);

        tokio::fs::remove_file(&cover_file).await.unwrap();
    }
}
