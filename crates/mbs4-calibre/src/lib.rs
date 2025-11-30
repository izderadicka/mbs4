use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

pub use crate::meta::EbookMetadata;
use crate::meta::parse_metadata;

pub mod meta;

const EBOOK_META_PROGRAM: &str = "ebook-meta";
const EBOOK_CONVERT_PROGRAM: &str = "ebook-convert";

async fn run_command(cmd: &mut tokio::process::Command) -> anyhow::Result<std::process::Output> {
    let output = cmd.output().await?;

    if output.status.success() {
        Ok(output)
    } else {
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        Err(anyhow::anyhow!(
            "{cmd:?} failed with status: {}",
            output.status
        ))
    }
}

pub async fn extract_metadata(
    path: impl AsRef<Path>,
    extract_cover: bool,
) -> anyhow::Result<EbookMetadata> {
    let path = path.as_ref();
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

    let output = run_command(&mut cmd).await?;

    let stdout = std::str::from_utf8(&output.stdout)?;
    let mut meta = parse_metadata(stdout);
    if let Some(cover_file) = cover_file {
        if tokio::fs::metadata(&cover_file).await.is_ok() {
            meta.cover_file = Some(cover_file.to_string_lossy().into());
        }
    }
    Ok(meta)
}

pub async fn convert(path: impl AsRef<Path>, format_ext: &str) -> anyhow::Result<PathBuf> {
    let path = path.as_ref();
    let mut cmd = tokio::process::Command::new(EBOOK_CONVERT_PROGRAM);
    let output_file = std::env::temp_dir().join(format!(
        "mbs4-ebook-{}.{}",
        uuid::Uuid::new_v4(),
        format_ext
    ));
    cmd.arg(path).arg(&output_file).stdin(Stdio::null());

    let _output = run_command(&mut cmd).await?;
    let file_meta = tokio::fs::metadata(&output_file).await?;
    if file_meta.is_file() && file_meta.len() > 0 {
        Ok(output_file)
    } else {
        Err(anyhow::anyhow!(
            "Failed to convert, missing or empty file: {output_file:?}"
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

    #[tokio::test]
    async fn test_convert() {
        let path = "../../test-data/samples/Holmes.epub";
        let converted = convert(path, "mobi").await.unwrap();
        assert!(converted.exists());
        tokio::fs::remove_file(&converted).await.unwrap();
    }
}
