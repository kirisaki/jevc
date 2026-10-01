use clap::{Parser, error::ErrorKind};
use jevc::{
    cli::Cli,
    commands::{emit, run},
    error::AppError,
};
use std::{io, process::ExitCode};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            return if error.print().is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(4)
            };
        }
        Err(_) => {
            // Clap errors can quote arbitrary argument values; never echo them.
            let error = AppError::new(
                "invalid_cli_usage",
                "Invalid CLI arguments; use --help for usage",
                1,
                false,
            );
            return if emit(&mut io::stdout().lock(), &error.response(), false).is_ok() {
                ExitCode::from(1)
            } else {
                ExitCode::from(4)
            };
        }
    };
    let mut stdout = io::stdout().lock();
    match run(&cli, &mut stdout).await {
        Ok(status) => ExitCode::from(status),
        Err(error) => {
            let status = error.exit_status;
            if !cli.quiet {
                eprintln!("{error}");
            }
            // A broken output stream cannot reliably carry an error envelope.
            if error.body.code == "output_error" {
                return ExitCode::from(4);
            }
            let pretty = cli.pretty && !cli.command.outputs_jsonl();
            if emit(&mut stdout, &error.response(), pretty).is_err() {
                return ExitCode::from(4);
            }
            ExitCode::from(status)
        }
    }
}
