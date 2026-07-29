use std::{io::IsTerminal, process::ExitCode};

use benchguard::{
    app,
    cli::{Cli, OutputFormat, requested_output_format},
    error::{BenchguardError, ExitClass},
    report::{JsonRenderer, Report, ReportRenderer},
};
use clap::{Parser, error::ErrorKind};

fn main() -> ExitCode {
    let arguments = std::env::args_os().collect::<Vec<_>>();
    let requested_format = requested_output_format(&arguments);
    let cli = match Cli::try_parse_from(arguments) {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
            return ExitCode::SUCCESS;
        }
        Err(error) if requested_format == OutputFormat::Json => {
            return operational_error(
                OutputFormat::Json,
                false,
                BenchguardError::InvalidArguments(error.to_string().trim().to_owned()),
            );
        }
        Err(error) => {
            let _ = error.print();
            return ExitCode::from(2);
        }
    };
    let output_format = cli.output_format();
    let color_enabled = cli.color.enabled(
        std::io::stdout().is_terminal(),
        std::env::var_os("NO_COLOR").is_some(),
        output_format,
    );
    match app::execute(cli) {
        Ok(ExitClass::Success) => ExitCode::SUCCESS,
        Ok(ExitClass::Regression) => ExitCode::from(1),
        Err(error) => operational_error(output_format, color_enabled, error),
    }
}

fn operational_error(
    output_format: OutputFormat,
    color_enabled: bool,
    error: BenchguardError,
) -> ExitCode {
    match output_format {
        OutputFormat::Human if color_enabled => eprintln!("\x1b[31merror: {error}\x1b[0m"),
        OutputFormat::Human => eprintln!("error: {error}"),
        OutputFormat::Json => {
            println!(
                "{}",
                JsonRenderer.render(&Report::operational_error(&error))
            );
        }
    }
    ExitCode::from(2)
}
