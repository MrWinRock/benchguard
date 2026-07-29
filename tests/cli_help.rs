use assert_cmd::Command;

fn help(args: &[&str]) -> String {
    let output = Command::cargo_bin("benchguard")
        .unwrap()
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .replace("\r\n", "\n")
        .replace("benchguard.exe", "benchguard")
}

// Catches blank/vague public-command descriptions and drift in generated
// global help/version behavior.
#[test]
fn top_level_help_is_the_complete_public_contract() {
    assert_eq!(
        help(&["help"]),
        "\
Record and enforce performance budgets for executable commands

Usage: benchguard <COMMAND>

Commands:
  record  Record or replace a performance baseline
  check   Measure a command and check it against a stored baseline
  list    List stored baselines without running commands
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
"
    );
}

// Catches missing descriptions, aliases, defaults, positional meaning, or
// budget/output guidance in record help.
#[test]
fn record_help_is_the_complete_public_contract() {
    assert_eq!(
        help(&["help", "record"]),
        "\
Record or replace a performance baseline

Usage: benchguard record [OPTIONS] <NAME> -- <TARGET>...

Arguments:
  <NAME>       Benchmark name in the baseline
  <TARGET>...  Executable followed by its exact arguments

Options:
  -r, --runs <RUNS>              Number of measured executions [default: 10]
  -w, --warmup <WARMUP>          Number of unmeasured warm-up executions [default: 2]
  -t, --timeout <TIMEOUT>        Maximum duration of each execution, such as 500ms or 2s
  -f, --file <FILE>              Path to the baseline JSON file [default: benchguard.json]
      --max-time <MAX_TIME>      Maximum allowed wall-time increase, such as +10%
      --max-cpu <MAX_CPU>        Maximum allowed CPU-time increase, such as +10%
      --max-memory <MAX_MEMORY>  Maximum allowed peak-memory increase, such as +10%
      --format <FORMAT>          Report format [default: human] [possible values: human, json]
  -h, --help                     Print help
  -V, --version                  Print version
"
    );
}

// Catches check help claiming record defaults instead of stored-setting
// overrides, or failing to explain the optional target.
#[test]
fn check_help_is_the_complete_public_contract() {
    assert_eq!(
        help(&["help", "check"]),
        "\
Measure a command and check it against a stored baseline

Usage: benchguard check [OPTIONS] <NAME> [-- <TARGET>...]

Arguments:
  <NAME>       Benchmark name in the baseline
  [TARGET]...  Optional executable and arguments; omit to use the stored command

Options:
  -r, --runs <RUNS>              Override the stored measured-run count
  -w, --warmup <WARMUP>          Override the stored warm-up count
  -t, --timeout <TIMEOUT>        Override the stored per-execution timeout
  -f, --file <FILE>              Path to the baseline JSON file [default: benchguard.json]
      --max-time <MAX_TIME>      Override the stored wall-time budget, such as +10%
      --max-cpu <MAX_CPU>        Override the stored CPU-time budget, such as +10%
      --max-memory <MAX_MEMORY>  Override the stored peak-memory budget, such as +10%
      --format <FORMAT>          Report format [default: human] [possible values: human, json]
  -h, --help                     Print help
  -V, --version                  Print version
"
    );
}

// Catches list help implying execution or omitting its file/output contracts.
#[test]
fn list_help_is_the_complete_public_contract() {
    assert_eq!(
        help(&["help", "list"]),
        "\
List stored baselines without running commands

Usage: benchguard list [OPTIONS]

Options:
  -f, --file <FILE>      Path to the baseline JSON file [default: benchguard.json]
      --format <FORMAT>  Report format [default: human] [possible values: human, json]
  -h, --help             Print help
  -V, --version          Print version
"
    );
}

// Catches exposing the documented version flag only at the top level instead
// of propagating it to each public subcommand.
#[test]
fn version_is_available_under_every_subcommand() {
    for (args, expected) in [
        (["record", "-V"].as_slice(), "benchguard-record 0.1.0\n"),
        (
            ["check", "--version"].as_slice(),
            "benchguard-check 0.1.0\n",
        ),
        (["list", "-V"].as_slice(), "benchguard-list 0.1.0\n"),
    ] {
        assert_eq!(help(args), expected);
    }
}
