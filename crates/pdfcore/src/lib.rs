//! Core library for `pdfcli`.
//!
//! This crate is intentionally small at bootstrap time. It provides a
//! centralized place for errors and for later PDF manipulation primitives.

use std::path::{Path, PathBuf};

use thiserror::Error;

/// Core error type for PDF operations.
#[derive(Debug, Error)]
pub enum PdfError {
    /// The given path did not exist.
    #[error("input file does not exist: {0}")]
    InputNotFound(PathBuf),
}

/// Validate that the input path exists and is a file.
pub fn validate_input_file(path: &Path) -> Result<(), PdfError> {
    if !path.exists() {
        return Err(PdfError::InputNotFound(path.to_path_buf()));
    }
    Ok(())
}
