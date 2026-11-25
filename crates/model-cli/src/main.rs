//! `model <command> [options]`: a small, scriptable client for a Model.
//!
//! Data commands (`query`, `impact`, `context`, `analysis`, `graph projection`,
//! `graph path`, `graph explain`, `graph communities`) print JSON to stdout, so
//! an agent (or a shell script) can consume them without parsing prose.
//! `graph report` prints Markdown. Progress and timings go to stderr. `detect`
//! and `build` print a short human summary
//! unless `--json` is given; `build --out <file>` writes the model itself
//! (use `--out -` for stdout).
//!
//! Set `MODEL_TIMING=1` to print how long loading a `model.json` took on each
//! invocation. That is the cost a stateless CLI pays on every call, and the
//! reason a long-lived client keeps the model in memory.

use std::env;

use anyhow::{Context, Result, bail};

use model_cli::commands;

const HELP: &str = "\
model — command-line client for a code model

Usage:
  model detect <root> [--json] [--pretty]
  model build <root> [--out <model.json|->] [--reindex never|if-missing|if-stale|always]
                     [--no-scip] [--no-cache] [--from-cache] [--scope <path> ...]
                     [--languages <lang> ...] [--index-command LANGUAGE=COMMAND ...]
                     [--fragment-cache <dir>] [--quiet] [--json] [--pretty]
  model query <model.json> [--key <key>] [--kind <kind>] [--name <substring>]
                           [--file <substring>] [--relation <kind> ...]
                           [--direction in|out|both] [--limit <n>] [--pretty]
  model context <model.json> --key <key> [--relation <kind> ...] [--limit <n>] [--pretty]
  model impact <model.json> --seed <key> ... [--direction callers|callees|both]
                            [--depth <n>] [--kinds <kind,kind>] [--limit <n>] [--pretty]
  model analysis <model.json> <metrics|code-metrics|cycles|unused|degrees>
                 [--kinds <kind,kind>] [--limit <n>] [--pretty]
  model graph projection <model.json> [--out <file|->] [--kinds <kind,kind>]
                         [--provenance <name>] [--no-communities] [--no-props]
                         [--no-sites] [--pretty]
  model graph report <model.json> [--out <file|->] [--top <n>]
  model graph path <model.json> --from <key> --to <key> [--kinds <kind,kind>]
                  [--depth <n>] [--direction callers|callees|both]
                  [--provenance <tree_sitter|scip|frontend|derived>] [--pretty]
  model graph explain <model.json> --key <key> [--kinds <kind,kind>]
                       [--limit <n>] [--pretty]
  model graph communities <model.json> [--top <n>] [--pretty]
  model help

Kinds are snake_case (`calls`, `references`, `method`, `http.endpoints`...).
A `--kind` filter matches the archetype or the fine kind of an entity.
`--limit 0` means no limit. MODEL_TIMING=1 prints load times on stderr.
";

fn main() -> Result<()> {
    let mut args = env::args();
    args.next(); // argv[0]: the binary's own path
    let command = args.next().context(HELP)?;
    match command.as_str() {
        "detect" => commands::detect(&mut args),
        "build" => commands::build(&mut args),
        "query" => commands::query(&mut args),
        "context" => commands::context(&mut args),
        "impact" => commands::impact(&mut args),
        "analysis" => commands::analysis(&mut args),
        "graph" => commands::graph(&mut args),
        "-h" | "--help" | "help" => {
            print!("{HELP}");
            Ok(())
        }
        other => bail!("unknown command {other:?}\n\n{HELP}"),
    }
}
