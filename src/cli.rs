use std::ffi::OsString;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    Human,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PercentBudget(pub f64);

impl FromStr for PercentBudget {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let value = input
            .trim()
            .trim_start_matches('+')
            .strip_suffix('%')
            .ok_or("budget must end in %")?
            .parse::<f64>()
            .map_err(|_| "budget must be a number")?;
        (value >= 0.0 && value.is_finite())
            .then_some(Self(value))
            .ok_or_else(|| "budget must be finite and non-negative".into())
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "benchguard",
    version = "0.1.1",
    propagate_version = true,
    about = "Record and enforce performance budgets for executable commands",
    after_help = "Notation:\n  <VALUE>   required\n  [VALUE]   optional\n  ...       repeatable\n\nExamples:\n  benchguard record npm-build --max-time +10% npm run build\n  benchguard check npm-build\n  benchguard record bun-build --max-time +10% bun run build\n  benchguard list --format json"
)]
pub struct Cli {
    #[arg(long, value_enum, default_value_t = ColorMode::Auto, global = true)]
    pub color: ColorMode,
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn output_format(&self) -> OutputFormat {
        match &self.command {
            Command::Record(args) => args.format,
            Command::Check(args) => args.format,
            Command::List(args) => args.format,
        }
    }
}

pub fn requested_output_format(args: &[OsString]) -> OutputFormat {
    let arguments = args
        .iter()
        .skip(1)
        .take_while(|argument| argument != &"--")
        .collect::<Vec<_>>();
    for (index, argument) in arguments.iter().enumerate() {
        let Some(argument) = argument.to_str() else {
            continue;
        };
        if argument == "--format=json" {
            return OutputFormat::Json;
        }
        if argument == "--format"
            && arguments
                .get(index + 1)
                .and_then(|value| value.to_str())
                .is_some_and(|value| value == "json")
        {
            return OutputFormat::Json;
        }
    }
    OutputFormat::Human
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Record or replace a performance baseline
    #[command(
        after_help = "Notation:\n  <VALUE>   required\n  [VALUE]   optional\n  ...       repeatable\n\nExamples:\n  benchguard record npm-build --max-time +10% npm run build\n  benchguard record bun-build --max-time +10% bun run build"
    )]
    Record(RecordArgs),
    /// Measure a command and check it against a stored baseline
    #[command(
        after_help = "Notation:\n  <VALUE>   required\n  [VALUE]   optional\n  ...       repeatable\n\nExamples:\n  benchguard check npm-build"
    )]
    Check(CheckArgs),
    /// List stored baselines without running commands
    #[command(
        after_help = "Notation:\n  <VALUE>   required\n  [VALUE]   optional\n  ...       repeatable\n\nExamples:\n  benchguard list --format json"
    )]
    List(ListArgs),
}

#[derive(Debug, Args)]
pub struct RunOptions {
    /// Number of measured executions
    #[arg(short = 'r', long, default_value_t = 10)]
    pub runs: u32,
    /// Number of unmeasured warm-up executions
    #[arg(short = 'w', long, default_value_t = 2)]
    pub warmup: u32,
    /// Maximum duration of each execution, such as 500ms or 2s
    #[arg(short = 't', long, value_parser = humantime::parse_duration)]
    pub timeout: Option<Duration>,
    /// Path to the baseline JSON file
    #[arg(short = 'f', long, default_value = "benchguard.json")]
    pub file: PathBuf,
}

#[derive(Debug, Args)]
pub struct RecordArgs {
    /// Benchmark name in the baseline
    pub name: String,
    #[command(flatten)]
    pub run: RunOptions,
    /// Maximum allowed wall-time increase, such as +10%
    #[arg(long)]
    pub max_time: Option<PercentBudget>,
    /// Maximum allowed CPU-time increase, such as +10%
    #[arg(long)]
    pub max_cpu: Option<PercentBudget>,
    /// Maximum allowed peak-memory increase, such as +10%
    #[arg(long)]
    pub max_memory: Option<PercentBudget>,
    /// Report format
    #[arg(long, value_enum, default_value = "human")]
    pub format: OutputFormat,
    /// Executable followed by its exact arguments
    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
    pub target: Vec<String>,
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    /// Benchmark name in the baseline
    pub name: String,
    /// Override the stored measured-run count
    #[arg(short = 'r', long)]
    pub runs: Option<u32>,
    /// Override the stored warm-up count
    #[arg(short = 'w', long)]
    pub warmup: Option<u32>,
    /// Override the stored per-execution timeout
    #[arg(short = 't', long, value_parser = humantime::parse_duration)]
    pub timeout: Option<Duration>,
    /// Path to the baseline JSON file
    #[arg(short = 'f', long, default_value = "benchguard.json")]
    pub file: PathBuf,
    /// Override the stored wall-time budget, such as +10%
    #[arg(long)]
    pub max_time: Option<PercentBudget>,
    /// Override the stored CPU-time budget, such as +10%
    #[arg(long)]
    pub max_cpu: Option<PercentBudget>,
    /// Override the stored peak-memory budget, such as +10%
    #[arg(long)]
    pub max_memory: Option<PercentBudget>,
    /// Report format
    #[arg(long, value_enum, default_value = "human")]
    pub format: OutputFormat,
    /// Optional executable and arguments; omit to use the stored command
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub target: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Path to the baseline JSON file
    #[arg(short = 'f', long, default_value = "benchguard.json")]
    pub file: PathBuf,
    /// Report format
    #[arg(long, value_enum, default_value = "human")]
    pub format: OutputFormat,
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::str::FromStr;

    use clap::Parser;

    use super::{Cli, ColorMode, Command, OutputFormat, PercentBudget, requested_output_format};

    // Catches accepting a unitless or malformed budget, losing the optional
    // leading plus sign, or permitting negative/non-finite percentages.
    #[test]
    fn percent_budget_requires_a_finite_non_negative_percentage() {
        assert_eq!(PercentBudget::from_str("+10%").unwrap().0, 10.0);
        assert_eq!(PercentBudget::from_str("0%").unwrap().0, 0.0);
        assert!(PercentBudget::from_str("10").is_err());
        assert!(PercentBudget::from_str("-1%").is_err());
        assert!(PercentBudget::from_str("NaN%").is_err());
        assert!(PercentBudget::from_str("inf%").is_err());
    }

    // Catches a parser that gives check the record defaults, which would
    // silently override the run settings stored in the baseline.
    #[test]
    fn omitted_check_options_remain_unspecified() {
        let cli = Cli::try_parse_from(["benchguard", "check", "startup"]).unwrap();
        let Command::Check(args) = cli.command else {
            panic!("expected check command");
        };

        assert_eq!(args.runs, None);
        assert_eq!(args.warmup, None);
        assert_eq!(args.timeout, None);
        assert_eq!(args.max_time, None);
        assert_eq!(args.format, OutputFormat::Human);
        assert!(args.target.is_empty());
    }

    #[test]
    fn target_delimiter_is_optional_and_leading_hyphens_are_preserved() {
        for record in [
            [
                "benchguard",
                "record",
                "build",
                "npm",
                "run",
                "build",
                "--silent",
            ]
            .as_slice(),
            [
                "benchguard",
                "record",
                "build",
                "--",
                "npm",
                "run",
                "build",
                "--silent",
            ]
            .as_slice(),
        ] {
            let cli = Cli::try_parse_from(record).unwrap();
            let Command::Record(args) = cli.command else {
                panic!("expected record");
            };
            assert_eq!(args.target, ["npm", "run", "build", "--silent"]);
        }

        for check in [
            [
                "benchguard",
                "check",
                "build",
                "bun",
                "run",
                "build",
                "--watch",
            ]
            .as_slice(),
            [
                "benchguard",
                "check",
                "build",
                "--",
                "bun",
                "run",
                "build",
                "--watch",
            ]
            .as_slice(),
        ] {
            let cli = Cli::try_parse_from(check).unwrap();
            let Command::Check(args) = cli.command else {
                panic!("expected check");
            };
            assert_eq!(args.target, ["bun", "run", "build", "--watch"]);
        }
    }

    #[test]
    fn color_accepts_documented_values_before_and_after_subcommands() {
        for (value, expected) in [
            ("auto", ColorMode::Auto),
            ("always", ColorMode::Always),
            ("never", ColorMode::Never),
        ] {
            for arguments in [
                ["benchguard", "--color", value, "check", "build"],
                ["benchguard", "check", "--color", value, "build"],
            ] {
                assert_eq!(Cli::try_parse_from(arguments).unwrap().color, expected);
            }
        }

        assert!(Cli::try_parse_from(["benchguard", "--color", "invalid", "list"]).is_err());
    }

    // Catches missing public options or a parser that reconstructs the target
    // instead of preserving the command argument vector.
    #[test]
    fn record_parses_budgets_format_and_target_boundaries() {
        let cli = Cli::try_parse_from([
            "benchguard",
            "record",
            "startup",
            "--max-time",
            "+10%",
            "--max-cpu",
            "5%",
            "--max-memory",
            "20%",
            "--format",
            "json",
            "--",
            "fixture",
            "two words",
        ])
        .unwrap();
        let Command::Record(args) = cli.command else {
            panic!("expected record command");
        };

        assert_eq!(args.max_time.unwrap().0, 10.0);
        assert_eq!(args.max_cpu.unwrap().0, 5.0);
        assert_eq!(args.max_memory.unwrap().0, 20.0);
        assert_eq!(args.format, OutputFormat::Json);
        assert_eq!(args.target, ["fixture", "two words"]);
    }

    // Catches a list parser that omits machine-readable output selection.
    #[test]
    fn list_accepts_json_format() {
        let cli = Cli::try_parse_from(["benchguard", "list", "--format", "json"]).unwrap();
        let Command::List(args) = cli.command else {
            panic!("expected list command");
        };

        assert_eq!(args.format, OutputFormat::Json);
    }

    // Catches scanning target arguments as BenchGuard options or overlooking
    // the equals form when clap cannot produce a parsed Cli.
    #[test]
    fn output_format_intent_stops_at_the_target_delimiter() {
        let args = ["benchguard", "record", "--format=json"].map(OsString::from);
        assert_eq!(requested_output_format(&args), OutputFormat::Json);

        let malformed_before_json =
            ["benchguard", "record", "--format", "--format=json"].map(OsString::from);
        assert_eq!(
            requested_output_format(&malformed_before_json),
            OutputFormat::Json
        );

        let target_args = [
            "benchguard",
            "record",
            "name",
            "--",
            "program",
            "--format",
            "json",
        ]
        .map(OsString::from);
        assert_eq!(requested_output_format(&target_args), OutputFormat::Human);
    }
}
