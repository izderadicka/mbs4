use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

#[cfg(unix)]
use tokio::process::Child;
use tokio::{
    io::{AsyncRead, AsyncReadExt as _},
    process::Command,
    time::timeout,
};

pub use crate::meta::EbookMetadata;
use crate::meta::parse_metadata;

pub mod lang;
pub mod meta;
pub mod ooffice;

const EBOOK_META_PROGRAM: &str = "ebook-meta";
const EBOOK_CONVERT_PROGRAM: &str = "ebook-convert";

const CONVERSION_TIMEOUT: u64 = 300;
const META_EXTRACTION_TIMEOUT: u64 = 120;

async fn wait_output_with_timeout(
    mut cmd: &mut Command,
    timeout_limit: Duration,
) -> anyhow::Result<std::process::Output> {
    async fn read_to_end<A: AsyncRead + Unpin>(pipe: &mut Option<A>) -> anyhow::Result<Vec<u8>> {
        let mut vec = Vec::new();
        if let Some(io) = pipe.as_mut() {
            io.read_to_end(&mut vec).await?;
        }
        Ok(vec)
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    set_new_process_group(&mut cmd);
    let mut child = cmd.spawn()?;

    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();

    let wait_finish = async move {
        match timeout(timeout_limit, child.wait()).await {
            Ok(Ok(status)) => Ok(status),
            Ok(Err(e)) => Err(anyhow::anyhow!("Command failed: {e}")),
            Err(_) => {
                terminate_process(child);
                Err(anyhow::anyhow!("Command timed out"))
            }
        }
    };
    let read_stdout = read_to_end(&mut stdout);
    let read_stderr = read_to_end(&mut stderr);

    let (status, stdout, stderr) = tokio::try_join!(wait_finish, read_stdout, read_stderr)?;

    if status.success() {
        Ok(std::process::Output {
            status,
            stdout,
            stderr,
        })
    } else {
        eprintln!("{}", String::from_utf8_lossy(&stderr));
        Err(anyhow::anyhow!("{cmd:?} failed with status: {}", status))
    }
}

async fn run_command(cmd: &mut Command, timeout: Duration) -> anyhow::Result<std::process::Output> {
    wait_output_with_timeout(cmd, timeout).await
}

#[cfg(unix)]
pub fn set_new_process_group(cmd: &mut Command) {
    // pre_exec is Unix-only and runs in the child after fork() and before execve().
    // It's the correct place to call setpgid so the child becomes a new process-group leader.
    unsafe {
        cmd.pre_exec(|| {
            // Pure nix: setpgid(pid=0, pgid=0) => set this process's PGID to its PID.
            // That makes the child the leader of a new process group.
            nix::unistd::setpgid(nix::unistd::Pid::from_raw(0), nix::unistd::Pid::from_raw(0))
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

            // Linux-only: set this process's death signal to SIGTERM
            #[cfg(target_os = "linux")]
            nix::sys::prctl::set_pdeathsig(nix::sys::signal::Signal::SIGTERM)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

            Ok(())
        });
    }
}

#[cfg(not(unix))]
pub fn set_new_process_group(cmd: &mut Command) {}

#[cfg(unix)]
fn terminate_process(mut child: Child) {
    fn kill_process(child: &Child, force: bool) {
        use nix::sys::signal::{Signal, kill, killpg};
        use nix::unistd::Pid;
        if let Some(pid) = child.id() {
            let pid: Pid = Pid::from_raw(pid as i32);
            let signal = if force {
                Signal::SIGKILL
            } else {
                Signal::SIGTERM
            };
            // child process should be group leader
            killpg(pid, signal).or_else(|_| kill(pid, signal)).ok();
        }
    }

    kill_process(&child, false);
    tokio::spawn(async move {
        if let Err(_) = timeout(Duration::from_secs(10), child.wait()).await {
            kill_process(&child, true);
        }
    });
}

#[cfg(not(unix))]
fn terminate_process(mut child: Child) {
    tokio::spawn(async move {
        child.start_kill().ok();
    });
}

pub async fn extract_metadata(
    path: impl AsRef<Path>,
    extract_cover: bool,
) -> anyhow::Result<EbookMetadata> {
    let path = path.as_ref();
    let mut cmd = Command::new(EBOOK_META_PROGRAM);
    let mut cover_file = None;

    cmd.arg(path).stdin(Stdio::null());

    if extract_cover {
        let tmp_name =
            std::env::temp_dir().join(format!("mbs4-cover-{}.jpg", uuid::Uuid::new_v4()));
        cmd.arg("--get-cover");
        cmd.arg(&tmp_name);
        cover_file = Some(tmp_name);
    }

    let output = run_command(&mut cmd, Duration::from_secs(META_EXTRACTION_TIMEOUT)).await?;

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
    let mut cmd = Command::new(EBOOK_CONVERT_PROGRAM);
    let output_file = std::env::temp_dir().join(format!(
        "mbs4-ebook-{}.{}",
        uuid::Uuid::new_v4(),
        format_ext
    ));
    cmd.arg(path).arg(&output_file).stdin(Stdio::null());

    let _output = run_command(&mut cmd, Duration::from_secs(CONVERSION_TIMEOUT)).await?;
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
        assert_eq!(metadata.language, Some("en".to_string()));
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
