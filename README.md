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
- Ubuntu/Debian: `apt-get install qpdf poppler-utils ghostscript`

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
cargo fmt
cargo clippy --workspace --all-targets
cargo test --workspace
```
