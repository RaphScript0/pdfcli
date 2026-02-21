# pdfcli

Production-grade Rust workspace scaffold for a PDF utility CLI.

## Workspace layout

- `crates/pdfcore`: library crate with shared core logic and error types
- `crates/pdfcli`: binary crate (the CLI) that depends on `pdfcore`
- `tests/fixtures`: sample fixture layout for integration tests

## Prerequisites (external tools)

This project is designed to wrap common PDF utilities. Depending on which
subcommands are added, you may need:

- **qpdf** (common operations like linearize, decrypt, inspect)
- **Poppler** tools (e.g. `pdftotext`, `pdfinfo`)
- **Ghostscript** (e.g. normalize, compress, render)

Install examples:

- macOS (Homebrew): `brew install qpdf poppler ghostscript`
- Ubuntu/Debian: `sudo apt-get update && sudo apt-get install -y qpdf poppler-utils ghostscript`
- Windows:
  - qpdf: `choco install qpdf` (Chocolatey)
  - poppler (pdftotext): `choco install poppler`
  - ghostscript: `choco install ghostscript`

Notes:
- CI runs unit tests without installing these tools; tool-dependent tests should skip when missing.
- Some tools may be named differently on different platforms/packagers.

## Build

From repo root:

```bash
cargo build
```

## Run

Bootstrap command (verifies the input file exists):

```bash
cargo run -p pdfcli -- validate ./some.pdf
```

If the file exists, it prints:

```text
OK: ./some.pdf
```

## Developer commands

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```
