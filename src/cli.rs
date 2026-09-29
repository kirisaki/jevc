use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "jevc", version, about = "LLM-first CLI for JEV", color = clap::ColorChoice::Never)]
pub struct Cli {
    /// Suppress routine stderr diagnostics.
    #[arg(long, global = true)]
    pub quiet: bool,
    /// Format JSON output with indentation (batch always emits compact JSONL).
    #[arg(long, global = true)]
    pub pretty: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Evaluate one JSON request using JEV.
    Decide(NetworkInput),
    /// Validate one JSON request locally, without configuration or network access.
    Validate(Input),
    /// Print a JSON Schema for the CLI protocol.
    Schema {
        #[arg(value_enum)]
        target: SchemaTarget,
    },
    /// Describe commands, configuration, and protocol as JSON.
    Describe,
    /// Print version information as JSON.
    Version,
    /// Evaluate JSONL sequentially, preserving order and continuing after errors.
    Batch(NetworkInput),
}

#[derive(Debug, Args)]
pub struct Input {
    /// Read from a file instead of stdin; '-' also means stdin.
    #[arg(long)]
    pub file: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct NetworkInput {
    #[command(flatten)]
    pub input: Input,
    /// HTTP request timeout in whole seconds.
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=86400))]
    pub timeout: u64,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SchemaTarget {
    Request,
    Response,
    Error,
    Validation,
    BatchRequest,
    BatchResponse,
}
