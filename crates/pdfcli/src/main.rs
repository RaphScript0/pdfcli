use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process,
};

use anyhow::{bail, Context};
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "pdfcli",
    version,
    about = "CLI wrapper around PDF utilities",
    after_help = "EXAMPLES:\n  pdfcli info file.pdf\n  pdfcli info file.pdf --json\n\n  pdfcli merge -o merged.pdf a.pdf b.pdf c.pdf\n\n  pdfcli split-pages input.pdf --out-dir pages/\n  pdfcli split-pages input.pdf --out-dir pages/ --pattern 'page-%d.pdf'\n\n  pdfcli extract-text input.pdf --stdout\n  pdfcli extract-text input.pdf -o out.txt\n\n  pdfcli rotate input.pdf -o rotated.pdf --degrees 90\n  pdfcli rotate input.pdf -o rotated.pdf --degrees 180 --pages '1-3'\n\n  pdfcli compress input.pdf -o small.pdf --preset ebook\n\nNOTES:\n  - Some commands rely on external tools (qpdf, pdftotext, ghostscript).\n  - Tool locations can be overridden with env vars: PDFCLI_QPDF, PDFCLI_PDFTOTEXT, PDFCLI_GS\n"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Print basic information about a PDF (pure Rust; no external tools).
    Info {
        /// Input PDF path
        input: PathBuf,

        /// Output machine-readable JSON
        #[arg(long)]
        json: bool,
    },

    /// Merge multiple PDFs into a single output PDF (requires qpdf).
    Merge {
        /// Output PDF path
        #[arg(short, long)]
        output: PathBuf,

        /// Overwrite output if it exists
        #[arg(long)]
        force: bool,

        /// Input PDFs (in order)
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
    },

    /// Split a PDF into one PDF per page (requires qpdf).
    SplitPages {
        /// Input PDF path
        input: PathBuf,

        /// Directory to write pages into
        #[arg(long)]
        out_dir: PathBuf,

        /// Output pattern, must contain %d (page number, 1-based)
        #[arg(long)]
        pattern: Option<String>,

        /// Overwrite existing files (best-effort; may still fail if tool refuses)
        #[arg(long)]
        force: bool,
    },

    /// Extract text from a PDF (requires pdftotext).
    ExtractText {
        /// Input PDF path
        input: PathBuf,

        /// Output text file path
        #[arg(short, long, conflicts_with = "stdout")]
        output: Option<PathBuf>,

        /// Write extracted text to stdout
        #[arg(long)]
        stdout: bool,

        /// Overwrite output if it exists
        #[arg(long)]
        force: bool,
    },

    /// Rotate pages in a PDF (requires qpdf).
    Rotate {
        /// Input PDF path
        input: PathBuf,

        /// Output PDF path
        #[arg(short, long)]
        output: PathBuf,

        /// Overwrite output if it exists
        #[arg(long)]
        force: bool,

        /// Rotation degrees (90, 180, 270)
        #[arg(long)]
        degrees: RotateDegrees,

        /// Page range (currently only a single inclusive range like '1-3')
        #[arg(long)]
        pages: Option<String>,
    },

    /// Compress/optimize a PDF (requires ghostscript).
    Compress {
        /// Input PDF path
        input: PathBuf,

        /// Output PDF path
        #[arg(short, long)]
        output: PathBuf,

        /// Overwrite output if it exists
        #[arg(long)]
        force: bool,

        /// Compression preset
        #[arg(long, value_enum, default_value_t = CompressPresetCli::Default)]
        preset: CompressPresetCli,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RotateDegrees {
    #[value(name = "90")]
    D90,
    #[value(name = "180")]
    D180,
    #[value(name = "270")]
    D270,
}

impl RotateDegrees {
    fn as_u16(self) -> u16 {
        match self {
            Self::D90 => 90,
            Self::D180 => 180,
            Self::D270 => 270,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CompressPresetCli {
    Screen,
    Ebook,
    Printer,
    Prepress,
    Default,
}

impl From<CompressPresetCli> for pdfcore::CompressPreset {
    fn from(value: CompressPresetCli) -> Self {
        match value {
            CompressPresetCli::Screen => Self::Screen,
            CompressPresetCli::Ebook => Self::Ebook,
            CompressPresetCli::Printer => Self::Printer,
            CompressPresetCli::Prepress => Self::Prepress,
            CompressPresetCli::Default => Self::Default,
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let code = match run(cli) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    };
    process::exit(code);
}

fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Commands::Info { input, json } => cmd_info(&input, json),
        Commands::Merge {
            output,
            force,
            inputs,
        } => cmd_merge(&inputs, &output, force),
        Commands::SplitPages {
            input,
            out_dir,
            pattern,
            force,
        } => cmd_split_pages(&input, &out_dir, pattern.as_deref(), force),
        Commands::ExtractText {
            input,
            output,
            stdout,
            force,
        } => cmd_extract_text(&input, output.as_deref(), stdout, force),
        Commands::Rotate {
            input,
            output,
            force,
            degrees,
            pages,
        } => cmd_rotate(&input, &output, force, degrees, pages.as_deref()),
        Commands::Compress {
            input,
            output,
            force,
            preset,
        } => cmd_compress(&input, &output, force, preset),
    }
}

fn cmd_info(input: &Path, json: bool) -> anyhow::Result<()> {
    let info =
        pdfcore::info(input).with_context(|| format!("reading pdf info: {}", input.display()))?;

    if json {
        let out = render_info_json(&info);
        print!("{out}");
    } else {
        println!("pages: {}", info.pages);
        if !info.metadata.is_empty() {
            println!("metadata:");
            for (k, v) in info.metadata {
                println!("  {k}: {v}");
            }
        }
    }

    Ok(())
}

fn cmd_merge(inputs: &[PathBuf], output: &Path, force: bool) -> anyhow::Result<()> {
    ensure_can_write_file(output, force)?;
    pdfcore::merge(inputs, output)
        .with_context(|| format!("merging {} file(s) into {}", inputs.len(), output.display()))?;
    eprintln!("wrote: {}", output.display());
    Ok(())
}

fn cmd_split_pages(
    input: &Path,
    out_dir: &Path,
    pattern: Option<&str>,
    force: bool,
) -> anyhow::Result<()> {
    pdfcore::validate_input_file(input)
        .with_context(|| format!("validating input: {}", input.display()))?;

    fs::create_dir_all(out_dir)
        .with_context(|| format!("creating out dir: {}", out_dir.display()))?;

    if !force {
        // best-effort: if directory non-empty, require --force
        if let Ok(mut it) = fs::read_dir(out_dir) {
            if it.next().is_some() {
                bail!(
                    "out-dir is not empty: {} (use --force to proceed)",
                    out_dir.display()
                );
            }
        }
    }

    pdfcore::split_pages(input, out_dir, pattern).with_context(|| {
        format!(
            "splitting {} into pages under {}",
            input.display(),
            out_dir.display()
        )
    })?;
    eprintln!("wrote pages to: {}", out_dir.display());
    Ok(())
}

fn cmd_extract_text(
    input: &Path,
    output: Option<&Path>,
    stdout: bool,
    force: bool,
) -> anyhow::Result<()> {
    if stdout {
        let text = pdfcore::extract_text(input, Option::<&Path>::None)
            .with_context(|| format!("extracting text from {}", input.display()))?;
        let mut w = io::stdout().lock();
        w.write_all(text.as_bytes())?;
        return Ok(());
    }

    let out = output.context("either -o/--output or --stdout is required")?;
    ensure_can_write_file(out, force)?;
    pdfcore::extract_text(input, Some(out)).with_context(|| {
        format!(
            "extracting text from {} into {}",
            input.display(),
            out.display()
        )
    })?;
    eprintln!("wrote: {}", out.display());
    Ok(())
}

fn cmd_rotate(
    input: &Path,
    output: &Path,
    force: bool,
    degrees: RotateDegrees,
    pages: Option<&str>,
) -> anyhow::Result<()> {
    ensure_can_write_file(output, force)?;
    let sel = pages
        .map(parse_page_selection)
        .transpose()
        .context("parsing --pages")?;

    pdfcore::rotate(input, output, degrees.as_u16(), sel)
        .with_context(|| format!("rotating {} -> {}", input.display(), output.display()))?;
    eprintln!("wrote: {}", output.display());
    Ok(())
}

fn cmd_compress(
    input: &Path,
    output: &Path,
    force: bool,
    preset: CompressPresetCli,
) -> anyhow::Result<()> {
    ensure_can_write_file(output, force)?;
    pdfcore::compress(input, output, preset.into()).with_context(|| {
        format!(
            "compressing {} -> {} (preset: {:?})",
            input.display(),
            output.display(),
            preset
        )
    })?;
    eprintln!("wrote: {}", output.display());
    Ok(())
}

fn ensure_can_write_file(path: &Path, force: bool) -> anyhow::Result<()> {
    if path.exists() && !force {
        bail!(
            "output already exists: {} (use --force to overwrite)",
            path.display()
        );
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating output dir: {}", parent.display()))?;
        }
    }
    Ok(())
}

fn parse_page_selection(s: &str) -> anyhow::Result<pdfcore::PageSelection> {
    // pdfcore currently supports All or a single inclusive range.
    let s = s.trim();
    let (start, end) = s
        .split_once('-')
        .ok_or_else(|| anyhow::anyhow!("expected format <start>-<end> (e.g. 1-3)"))?;

    let start: u32 = start.trim().parse().context("parsing start page")?;
    let end: u32 = end.trim().parse().context("parsing end page")?;

    if start == 0 || end == 0 {
        bail!("pages are 1-based; got {start}-{end}");
    }
    if start > end {
        bail!("page range start must be <= end; got {start}-{end}");
    }

    Ok(pdfcore::PageSelection::Range { start, end })
}

fn render_info_json(info: &pdfcore::PdfInfo) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    out.push_str("{\n");
    let _ = writeln!(&mut out, "  \"pages\": {},", info.pages);

    out.push_str("  \"metadata\": {");
    if info.metadata.is_empty() {
        out.push_str("}\n");
    } else {
        out.push('\n');
        let mut first = true;
        for (k, v) in &info.metadata {
            if !first {
                out.push_str(",\n");
            }
            first = false;
            let _ = write!(&mut out, "    {}: {}", json_string(k), json_string(v));
        }
        out.push_str("\n  }\n");
    }

    out.push_str("}\n");
    out
}

fn json_string(s: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(&mut out, "\\u{:04x}", c as u32);
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}
