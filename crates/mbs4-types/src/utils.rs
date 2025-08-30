use std::ffi::OsStr;

pub mod naming;

pub fn file_ext(path: impl AsRef<OsStr>) -> Option<String> {
    std::path::Path::new(path.as_ref())
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
}
