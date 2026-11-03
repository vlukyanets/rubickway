//! Why RubickWay stopped with an error (the application itself is in `standalone`).

use thiserror::Error;

use crate::render::RenderError;
use crate::render::egl::EglError;
use crate::server::ServerError;

/// Why the application stopped with an error. Messages follow `contracts/cli.md`.
#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Drm(#[from] crate::backend::drm::DrmError),
    #[error(transparent)]
    Server(#[from] ServerError),
    #[error("rendering failed: {0}")]
    Render(String),
    #[error("event loop: {0}")]
    Loop(String),
}

impl From<EglError> for AppError {
    fn from(err: EglError) -> Self {
        AppError::Render(err.to_string())
    }
}

impl From<RenderError> for AppError {
    fn from(err: RenderError) -> Self {
        AppError::Render(err.to_string())
    }
}
