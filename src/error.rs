use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("project detection failed: {0}")]
    ProjectDetection(#[from] anyhow::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
