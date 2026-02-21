//! Core library for `pdfcli`.
//!
//! This crate provides:
//! - **Pure Rust** PDF inspection (`info`) using [`lopdf`].
//! - Thin wrappers around external tools (`qpdf`, `pdftotext`, `ghostscript`).
//!
//! External tools are detected at runtime (PATH search) and may be overridden
//! via env vars:
//! - `PDFCLI_QPDF`
//! - `PDFCLI_PDFTOTEXT`
//! - `PDFCLI_GS`

use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fmt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use thiserror::Error;

/// Convenient result type for this crate.
pub type Result<T> = std::result::Result<T, PdfError>;

/// Errors returned by `pdfcore`.
#[derive(Debug, Error)]
pub enum PdfError {
    /// The given path did not exist.
    #[error("input file does not exist: {0}")]
    InputNotFound(PathBuf),

    /// Failed to read/parse PDF in pure Rust.
    #[error("failed to parse pdf: {path}: {source}")]
    PdfParse {
        path: PathBuf,
        #[source]
        source: lopdf::Error,
    },

    /// External tool required but missing.
    #[error("required tool not found: {tool}\n\n{hint}")]
    MissingTool { tool: &'static str, hint: String },

    /// External tool failed.
    #[error(
        "tool execution failed: {tool}\ncommand: {command}\nstatus: {status}\nstdout: {stdout}\nstderr: {stderr}"
    )]
    ToolFailed {
        tool: &'static str,
        command: String,
        status: i32,
        stdout: String,
        stderr: String,
    },

    /// IO error.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Invalid argument.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}

/// Validates that the input path exists (and is a file).
pub fn validate_input_file(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(PdfError::InputNotFound(path.to_path_buf()));
    }
    Ok(())
}

/// Basic information about a PDF file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfInfo {
    /// Total number of pages.
    pub pages: u32,
    /// Document metadata. Keys are typical PDF Info dict entries (e.g. `Title`).
    pub metadata: BTreeMap<String, String>,
}

/// Read PDF info **without external tools**.
///
/// Returns at least page count; metadata may be empty.
pub fn info(path: impl AsRef<Path>) -> Result<PdfInfo> {
    let path = path.as_ref();
    validate_input_file(path)?;

    let doc = lopdf::Document::load(path).map_err(|source| PdfError::PdfParse {
        path: path.to_path_buf(),
        source,
    })?;

    let pages = u32::try_from(doc.get_pages().len())
        .map_err(|_| PdfError::InvalidArgument("page count overflow".to_string()))?;

    let mut metadata = BTreeMap::new();
    if let Ok(trailer) = doc.trailer.get(b"Info") {
        if let Ok(info_ref) = trailer.as_reference() {
            if let Ok(obj) = doc.get_object(info_ref) {
                if let Ok(dict) = obj.as_dict() {
                    for (k, v) in dict {
                        let key = String::from_utf8_lossy(k).to_string();
                        if let Some(val) = pdf_object_to_string(v) {
                            metadata.insert(key, val);
                        }
                    }
                }
            }
        }
    }

    Ok(PdfInfo { pages, metadata })
}

fn pdf_object_to_string(obj: &lopdf::Object) -> Option<String> {
    match obj {
        lopdf::Object::String(bytes, _) => Some(String::from_utf8_lossy(bytes).to_string()),
        lopdf::Object::Name(name) => Some(String::from_utf8_lossy(name).to_string()),
        lopdf::Object::Integer(i) => Some(i.to_string()),
        lopdf::Object::Real(f) => Some(f.to_string()),
        lopdf::Object::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Compression preset for `compress`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressPreset {
    /// `/screen`
    Screen,
    /// `/ebook`
    Ebook,
    /// `/printer`
    Printer,
    /// `/prepress`
    Prepress,
    /// `/default`
    Default,
}

impl CompressPreset {
    fn as_gs_setting(self) -> &'static str {
        match self {
            Self::Screen => "/screen",
            Self::Ebook => "/ebook",
            Self::Printer => "/printer",
            Self::Prepress => "/prepress",
            Self::Default => "/default",
        }
    }
}

/// Page selection for operations like rotate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageSelection {
    /// Apply to all pages.
    All,
    /// Apply to a 1-based inclusive page range.
    Range { start: u32, end: u32 },
}

impl PageSelection {
    fn to_qpdf_arg(&self) -> Option<String> {
        match self {
            Self::All => None,
            Self::Range { start, end } => Some(format!("{start}-{end}")),
        }
    }
}

/// Merge multiple PDFs into one using `qpdf`.
pub fn merge(inputs: &[impl AsRef<Path>], output: impl AsRef<Path>) -> Result<()> {
    if inputs.is_empty() {
        return Err(PdfError::InvalidArgument(
            "merge requires at least one input".to_string(),
        ));
    }
    for p in inputs {
        validate_input_file(p.as_ref())?;
    }

    let qpdf = find_tool(Tool::Qpdf)?;
    let mut cmd = Command::new(qpdf);
    cmd.arg("--empty")
        .arg("--pages")
        .args(inputs.iter().map(|p| p.as_ref().as_os_str()))
        .arg("--")
        .arg(output.as_ref().as_os_str());

    run_tool(Tool::Qpdf, cmd)
}

/// Split a PDF into pages using `qpdf`.
///
/// If `pattern` is provided it should include `%d` which will be replaced with the
/// page number (1-based), e.g. `"out-%d.pdf"`.
///
/// If `pattern` is `None`, pages are written into `out_dir` as `page-<n>.pdf`.
pub fn split_pages(
    input: impl AsRef<Path>,
    out_dir: impl AsRef<Path>,
    pattern: Option<&str>,
) -> Result<()> {
    validate_input_file(input.as_ref())?;
    let qpdf = find_tool(Tool::Qpdf)?;

    let pattern = if let Some(p) = pattern {
        p.to_string()
    } else {
        let mut p = PathBuf::from(out_dir.as_ref());
        p.push("page-%d.pdf");
        p.to_string_lossy().to_string()
    };

    if !pattern.contains("%d") {
        return Err(PdfError::InvalidArgument(
            "split_pages pattern must contain %d".to_string(),
        ));
    }

    let mut cmd = Command::new(qpdf);
    cmd.arg("--split-pages")
        .arg(input.as_ref().as_os_str())
        .arg(pattern);

    run_tool(Tool::Qpdf, cmd)
}

/// Extract text using Poppler's `pdftotext`.
///
/// If `output` is `None`, writes to stdout.
pub fn extract_text(input: impl AsRef<Path>, output: Option<impl AsRef<Path>>) -> Result<String> {
    validate_input_file(input.as_ref())?;
    let pdftotext = find_tool(Tool::Pdftotext)?;

    let mut cmd = Command::new(pdftotext);
    cmd.arg(input.as_ref().as_os_str());
    if let Some(out) = output {
        cmd.arg(out.as_ref().as_os_str());
        run_tool(Tool::Pdftotext, cmd)?;
        Ok(String::new())
    } else {
        cmd.arg("-");
        let out = run_tool_capture(Tool::Pdftotext, cmd)?;
        Ok(out)
    }
}

/// Rotate pages using `qpdf`.
///
/// `degrees` must be one of: 0, 90, 180, 270.
/// `pages` defaults to `All`.
pub fn rotate(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    degrees: u16,
    pages: Option<PageSelection>,
) -> Result<()> {
    validate_input_file(input.as_ref())?;
    let qpdf = find_tool(Tool::Qpdf)?;

    if !matches!(degrees, 0 | 90 | 180 | 270) {
        return Err(PdfError::InvalidArgument(
            "degrees must be 0, 90, 180, or 270".to_string(),
        ));
    }

    let pages = pages.unwrap_or(PageSelection::All);
    let mut rotate_arg = format!("+{degrees}");
    if let Some(sel) = pages.to_qpdf_arg() {
        rotate_arg.push(':');
        rotate_arg.push_str(&sel);
    }

    let mut cmd = Command::new(qpdf);
    cmd.arg("--rotate")
        .arg(rotate_arg)
        .arg(input.as_ref().as_os_str())
        .arg(output.as_ref().as_os_str());

    run_tool(Tool::Qpdf, cmd)
}

/// Compress/optimize a PDF using Ghostscript.
pub fn compress(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    preset: CompressPreset,
) -> Result<()> {
    validate_input_file(input.as_ref())?;
    let gs = find_tool(Tool::Ghostscript)?;

    let mut cmd = Command::new(gs);
    cmd.arg("-sDEVICE=pdfwrite")
        .arg("-dCompatibilityLevel=1.4")
        .arg(format!("-dPDFSETTINGS={}", preset.as_gs_setting()))
        .arg("-dNOPAUSE")
        .arg("-dBATCH")
        .arg("-dSAFER")
        .arg(format!("-sOutputFile={}", output.as_ref().display()))
        .arg(input.as_ref().as_os_str());

    run_tool(Tool::Ghostscript, cmd)
}

#[derive(Debug, Clone, Copy)]
enum Tool {
    Qpdf,
    Pdftotext,
    Ghostscript,
}

impl Tool {
    fn name(self) -> &'static str {
        match self {
            Self::Qpdf => "qpdf",
            Self::Pdftotext => "pdftotext",
            Self::Ghostscript => "ghostscript",
        }
    }

    fn env_override(self) -> &'static str {
        match self {
            Self::Qpdf => "PDFCLI_QPDF",
            Self::Pdftotext => "PDFCLI_PDFTOTEXT",
            Self::Ghostscript => "PDFCLI_GS",
        }
    }

    fn default_exe_names(self) -> &'static [&'static str] {
        match self {
            Self::Qpdf => &["qpdf"],
            Self::Pdftotext => &["pdftotext"],
            Self::Ghostscript => &["gs", "gswin64c", "gswin32c"],
        }
    }

    fn install_hint(self) -> String {
        let tool = self.name();
        let mac = match self {
            Self::Ghostscript => "brew install ghostscript",
            _ => &format!("brew install {tool}"),
        };
        let ubuntu = match self {
            Self::Pdftotext => "sudo apt-get update && sudo apt-get install -y poppler-utils",
            Self::Ghostscript => "sudo apt-get update && sudo apt-get install -y ghostscript",
            Self::Qpdf => "sudo apt-get update && sudo apt-get install -y qpdf",
        };
        let windows = match self {
            Self::Ghostscript => "choco install ghostscript OR scoop install ghostscript",
            Self::Pdftotext => "choco install poppler OR scoop install poppler",
            Self::Qpdf => "choco install qpdf OR scoop install qpdf",
        };

        format!(
            "Set {} to a full path, or install:\n  macOS: {}\n  Ubuntu/Debian: {}\n  Windows: {}",
            self.env_override(),
            mac,
            ubuntu,
            windows
        )
    }
}

fn find_tool(tool: Tool) -> Result<PathBuf> {
    if let Some(val) = std::env::var_os(tool.env_override()) {
        let p = PathBuf::from(val);
        if p.exists() {
            return Ok(p);
        }
        return Err(PdfError::MissingTool {
            tool: tool.name(),
            hint: format!(
                "{} was set to {}, but that path does not exist.\n\n{}",
                tool.env_override(),
                p.display(),
                tool.install_hint()
            ),
        });
    }

    for exe in tool.default_exe_names() {
        if let Ok(p) = which::which(exe) {
            return Ok(p);
        }
    }

    Err(PdfError::MissingTool {
        tool: tool.name(),
        hint: tool.install_hint(),
    })
}

fn run_tool(tool: Tool, mut cmd: Command) -> Result<()> {
    let command_str = command_to_string(&cmd);
    let out = cmd.output()?;
    if out.status.success() {
        return Ok(());
    }

    Err(PdfError::ToolFailed {
        tool: tool.name(),
        command: command_str,
        status: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
    })
}

fn run_tool_capture(tool: Tool, mut cmd: Command) -> Result<String> {
    let command_str = command_to_string(&cmd);
    let out: Output = cmd.output()?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).to_string());
    }

    Err(PdfError::ToolFailed {
        tool: tool.name(),
        command: command_str,
        status: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
    })
}

fn command_to_string(cmd: &Command) -> String {
    let prog = cmd.get_program().to_string_lossy();
    let args = cmd
        .get_args()
        .map(shell_escape)
        .collect::<Vec<_>>()
        .join(" ");
    format!("{prog} {args}").trim().to_string()
}

fn shell_escape(s: &OsStr) -> String {
    let s = s.to_string_lossy();
    if s.contains(' ') {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

impl fmt::Display for PageSelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => write!(f, "all"),
            Self::Range { start, end } => write!(f, "{start}-{end}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_reads_page_count_from_minimal_pdf() {
        let mut doc = lopdf::Document::with_version("1.4");

        // Build a minimal, well-formed, 1-page PDF.
        let pages_id = doc.new_object_id();
        let page_id = doc.new_object_id();
        let catalog_id = doc.new_object_id();

        doc.objects.insert(
            pages_id,
            lopdf::Object::Dictionary(lopdf::Dictionary::from_iter([
                (b"Type".to_vec(), lopdf::Object::Name(b"Pages".to_vec())),
                (
                    b"Kids".to_vec(),
                    lopdf::Object::Array(vec![lopdf::Object::Reference(page_id)]),
                ),
                (b"Count".to_vec(), lopdf::Object::Integer(1)),
            ])),
        );

        doc.objects.insert(
            page_id,
            lopdf::Object::Dictionary(lopdf::Dictionary::from_iter([
                (b"Type".to_vec(), lopdf::Object::Name(b"Page".to_vec())),
                (b"Parent".to_vec(), lopdf::Object::Reference(pages_id)),
                (
                    b"MediaBox".to_vec(),
                    lopdf::Object::Array(vec![
                        lopdf::Object::Integer(0),
                        lopdf::Object::Integer(0),
                        lopdf::Object::Integer(612),
                        lopdf::Object::Integer(792),
                    ]),
                ),
            ])),
        );

        doc.objects.insert(
            catalog_id,
            lopdf::Object::Dictionary(lopdf::Dictionary::from_iter([
                (b"Type".to_vec(), lopdf::Object::Name(b"Catalog".to_vec())),
                (b"Pages".to_vec(), lopdf::Object::Reference(pages_id)),
            ])),
        );

        doc.trailer
            .set(b"Root", lopdf::Object::Reference(catalog_id));

        let f = tempfile::NamedTempFile::new().unwrap();
        doc.save(f.path()).unwrap();

        let i = info(f.path()).unwrap();
        assert_eq!(i.pages, 1);
    }

    fn tool_available(tool: Tool) -> bool {
        find_tool(tool).is_ok()
    }

    #[test]
    fn extract_text_skips_if_missing_pdftotext() {
        if !tool_available(Tool::Pdftotext) {
            eprintln!("skipping: pdftotext missing");
            return;
        }

        // Build a minimal PDF; pdftotext may output nothing but should not error.
        let mut doc = lopdf::Document::with_version("1.4");

        let pages_id = doc.new_object_id();
        let page_id = doc.new_object_id();
        let catalog_id = doc.new_object_id();

        doc.objects.insert(
            pages_id,
            lopdf::Object::Dictionary(lopdf::Dictionary::from_iter([
                (b"Type".to_vec(), lopdf::Object::Name(b"Pages".to_vec())),
                (
                    b"Kids".to_vec(),
                    lopdf::Object::Array(vec![lopdf::Object::Reference(page_id)]),
                ),
                (b"Count".to_vec(), lopdf::Object::Integer(1)),
            ])),
        );

        doc.objects.insert(
            page_id,
            lopdf::Object::Dictionary(lopdf::Dictionary::from_iter([
                (b"Type".to_vec(), lopdf::Object::Name(b"Page".to_vec())),
                (b"Parent".to_vec(), lopdf::Object::Reference(pages_id)),
                (
                    b"MediaBox".to_vec(),
                    lopdf::Object::Array(vec![
                        lopdf::Object::Integer(0),
                        lopdf::Object::Integer(0),
                        lopdf::Object::Integer(612),
                        lopdf::Object::Integer(792),
                    ]),
                ),
            ])),
        );

        doc.objects.insert(
            catalog_id,
            lopdf::Object::Dictionary(lopdf::Dictionary::from_iter([
                (b"Type".to_vec(), lopdf::Object::Name(b"Catalog".to_vec())),
                (b"Pages".to_vec(), lopdf::Object::Reference(pages_id)),
            ])),
        );

        doc.trailer
            .set(b"Root", lopdf::Object::Reference(catalog_id));

        let f = tempfile::NamedTempFile::new().unwrap();
        doc.save(f.path()).unwrap();

        let _ = extract_text(f.path(), Option::<&std::path::Path>::None).unwrap();
    }
}
