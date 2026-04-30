use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("project detection failed: {0}")]
    ProjectDetection(#[from] anyhow::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HOME environment variable is not set")]
    MissingHome,

    #[error("`git` not found on PATH")]
    GitNotFound,

    #[error("`git clone` failed with exit code {code}")]
    GitCloneFailed { code: i32 },

    #[error("destination exists as a regular file: {path}")]
    DestIsFile { path: std::path::PathBuf },

    #[error("destination directory is not empty: {path}")]
    DestNotEmpty { path: std::path::PathBuf },

    #[error("failed to read destination at {path}: {source}")]
    DestReadFailed {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
}
