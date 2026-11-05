//! `ignite-lsp-types --meta-model PATH --root METHOD... --out PATH`: writes the Ignite module of
//! LSP message types reachable from the root methods. Exit 0 written, 1 a reachable type is
//! refused (the message names it), 2 a usage error or a malformed meta model, 3 a file error.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use ignite_lsp_types::{Error, generate};

#[derive(Parser)]
#[command(
    name = "ignite-lsp-types",
    about = "Generate the Ignite module of LSP message types from metaModel.json"
)]
struct Args {
    /// The LSP meta model (`docs/lsp/metaModel.json`).
    #[arg(long)]
    meta_model: PathBuf,
    /// A request or notification whose params and result types are generated; repeatable.
    #[arg(long, required = true)]
    root: Vec<String>,
    /// The Ignite module to write.
    #[arg(long)]
    out: PathBuf,
    /// The module's package.
    #[arg(long, default_value = "examples.lsp.lsp_types")]
    package: String,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let text = match std::fs::read_to_string(&args.meta_model) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("ignite-lsp-types: {}: {error}", args.meta_model.display());
            return ExitCode::from(3);
        }
    };
    let module = match generate(&text, &args.root, &args.package) {
        Ok(module) => module,
        Err(error) => {
            eprintln!("ignite-lsp-types: {error}");
            return ExitCode::from(match error {
                Error::Refused(_) => 1,
                Error::Malformed(_) => 2,
            });
        }
    };
    if let Err(error) = std::fs::write(&args.out, module) {
        eprintln!("ignite-lsp-types: {}: {error}", args.out.display());
        return ExitCode::from(3);
    }
    ExitCode::SUCCESS
}
