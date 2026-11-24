//! The CLI's subcommands. All data output is JSON on stdout; progress and
//! timings go to stderr so the output can be piped without cleanup. The query
//! logic lives in `model_cli::api`, shared with the MCP server.

use std::env::Args;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use model_analysis::Direction as ImpactDirection;
use model_core::RelationKind;
use model_factory::index::{CustomIndex, Reindex};
use model_factory::{BuildOptions, Progress};
use serde::Serialize;

use crate::api::{self, Query};
use crate::load;
use crate::output::{self, AnalysisOut, Side};

fn next(args: &mut Args, usage: &str) -> Result<String> {
    args.next()
        .with_context(|| format!("{usage}\nRun `model --help`."))
}

fn shown(root: &Path) -> String {
    if root.as_os_str().is_empty() {
        ".".into()
    } else {
        root.display().to_string()
    }
}

#[derive(Serialize)]
struct DetectLang {
    language: String,
    files: usize,
    project_roots: Vec<String>,
}

#[derive(Serialize)]
struct DetectJson {
    root: String,
    languages: Vec<DetectLang>,
}

pub fn detect(args: &mut Args) -> Result<()> {
    let path = PathBuf::from(next(args, "usage: model detect <root> [--json]")?);
    let mut as_json = false;
    let mut pretty = false;
    for a in args {
        match a.as_str() {
            "--json" => as_json = true,
            "--pretty" => pretty = true,
            "-h" | "--help" => {
                println!("usage: model detect <root> [--json] [--pretty]");
                return Ok(());
            }
            _ => bail!("unexpected argument {a:?}"),
        }
    }
    let detection = model_factory::detect::detect(&path)?;
    let root = shown(&detection.root);
    if as_json {
        let out = DetectJson {
            root,
            languages: detection
                .languages
                .iter()
                .map(|l| DetectLang {
                    language: l.language.as_str().to_string(),
                    files: l.files,
                    project_roots: l.project_roots.iter().map(|r| shown(r)).collect(),
                })
                .collect(),
        };
        return output::print_json(&out, pretty);
    }
    println!("{root}");
    if detection.languages.is_empty() {
        println!("no language detected");
    }
    for l in &detection.languages {
        println!(
            "{}: {} files, project roots {}",
            l.language,
            l.files,
            l.project_roots
                .iter()
                .map(|r| shown(r))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(())
}

#[derive(Serialize)]
struct BuildJson {
    root: String,
    languages: Vec<String>,
    entities: usize,
    relations: usize,
    sites: usize,
    fragments_reused: usize,
    fragments_extracted: usize,
    indexes: usize,
    warnings: Vec<String>,
    elapsed_ms: u64,
    output: Option<String>,
}

pub fn build(args: &mut Args) -> Result<()> {
    let root = PathBuf::from(next(args, "usage: model build <root> [options]")?);
    let mut opts = BuildOptions::default();
    opts.index.reindex = Reindex::Never;
    if let Some(c) = CustomIndex::from_env().map_err(anyhow::Error::msg)? {
        opts.index.set_custom(c);
    }
    let mut out: Option<PathBuf> = None;
    let mut quiet = false;
    let mut as_json = false;
    let mut pretty = false;
    let mut scope = Vec::new();
    let mut languages = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => out = Some(PathBuf::from(next(args, "--out <model.json|->")?)),
            "--reindex" => {
                opts.index.reindex =
                    match next(args, "--reindex never|if-missing|if-stale|always")?.as_str() {
                        "never" => Reindex::Never,
                        "if-missing" => Reindex::IfMissing,
                        "if-stale" => Reindex::IfStale,
                        "always" => Reindex::Always,
                        other => bail!("--reindex never|if-missing|if-stale|always, not {other:?}"),
                    }
            }
            "--scope" => scope.push(next(args, "--scope <path>")?),
            "--languages" => languages.push(next(args, "--languages <lang>")?),
            "--index-command" => {
                let spec = next(args, "--index-command LANGUAGE=COMMAND")?;
                opts.index
                    .set_custom(CustomIndex::parse(&spec).map_err(anyhow::Error::msg)?);
            }
            "--fragment-cache" => {
                opts.fragment_cache_dir =
                    Some(PathBuf::from(next(args, "--fragment-cache <dir>")?));
            }
            "--no-scip" => opts.scip = false,
            "--no-cache" => opts.cache = None,
            "--from-cache" => opts.from_cache = true,
            "--quiet" => quiet = true,
            "--json" => as_json = true,
            "--pretty" => pretty = true,
            "-h" | "--help" => {
                println!(
                    "usage: model build <root> [--out <model.json|->] [--reindex ...] [--no-scip] [--no-cache] [--from-cache] [--scope <path> ...] [--languages <lang> ...] [--index-command L=C ...] [--fragment-cache <dir>] [--quiet] [--json] [--pretty]"
                );
                return Ok(());
            }
            _ => bail!("unexpected argument {a:?}"),
        }
    }
    if !scope.is_empty() {
        opts.scope = Some(scope);
    }
    if !languages.is_empty() {
        opts.languages = Some(languages);
    }
    if as_json && out.as_deref() == Some(Path::new("-")) {
        bail!("--json and --out - both want stdout: drop one");
    }

    let start = Instant::now();
    let built = model_factory::build(&root, &opts, &|p: Progress| {
        if !quiet {
            eprintln!(
                "[{:>3}%] {}{}",
                p.pct,
                p.step,
                p.detail.map(|d| format!(": {d}")).unwrap_or_default()
            );
        }
    })?;
    let elapsed = start.elapsed();
    load::note("build", elapsed);
    for w in &built.warnings {
        eprintln!("warning: {w}");
    }
    let mem = built.model.memory();
    let summary = BuildJson {
        root: shown(&root),
        languages: built
            .detection
            .languages
            .iter()
            .map(|l| l.language.as_str().to_string())
            .collect(),
        entities: mem.entities,
        relations: mem.relations,
        sites: mem.sites,
        fragments_reused: built.fragments.reused,
        fragments_extracted: built.fragments.extracted,
        indexes: built.indexes.len(),
        warnings: built.warnings,
        elapsed_ms: elapsed.as_millis() as u64,
        output: out.as_ref().map(|p| {
            if p == Path::new("-") {
                "stdout".into()
            } else {
                p.display().to_string()
            }
        }),
    };
    if let Some(path) = &out {
        load::write_json(path, &built.model)?;
    }
    if as_json {
        return output::print_json(&summary, pretty);
    }
    println!(
        "{}: {} entities, {} relations, {} sites in {} ms",
        summary.root, summary.entities, summary.relations, summary.sites, summary.elapsed_ms
    );
    println!(
        "fragments: {} reused, {} parsed; {} indexes",
        summary.fragments_reused, summary.fragments_extracted, summary.indexes
    );
    if let Some(path) = &summary.output {
        println!("wrote {path}");
    }
    Ok(())
}

fn parse_side(value: &str) -> Result<Option<Side>> {
    match value {
        "in" => Ok(Some(Side::In)),
        "out" => Ok(Some(Side::Out)),
        "both" => Ok(None),
        other => bail!("expected in|out|both, not {other:?}"),
    }
}

pub fn query(args: &mut Args) -> Result<()> {
    let path = PathBuf::from(next(args, "usage: model query <model.json> [filters]")?);
    let mut key = None;
    let mut kind = None;
    let mut name = None;
    let mut file = None;
    let mut relations = Vec::new();
    let mut side = None;
    let mut limit = 50;
    let mut pretty = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--key" => key = Some(next(args, "--key <key>")?),
            "--kind" => kind = Some(next(args, "--kind <kind>")?),
            "--name" => name = Some(next(args, "--name <substring>")?),
            "--file" => file = Some(next(args, "--file <substring>")?),
            "--relation" => relations.extend(output::kinds(&next(args, "--relation <kind>")?)),
            "--direction" => side = parse_side(&next(args, "--direction in|out|both")?)?,
            "--limit" => limit = next(args, "--limit <n>")?.parse()?,
            "--pretty" => pretty = true,
            "-h" | "--help" => {
                println!(
                    "usage: model query <model.json> [--key <key>] [--kind <kind>] [--name <substring>] [--file <substring>] [--relation <kind> ...] [--direction in|out|both] [--limit <n>] [--pretty]"
                );
                return Ok(());
            }
            _ => bail!("unexpected argument {a:?}"),
        }
    }
    let model = load::model(&path)?;
    let out = api::query(
        &model,
        Query {
            key: key.as_deref(),
            kind: kind.as_deref(),
            name: name.as_deref(),
            file: file.as_deref(),
            relations,
            side,
            limit,
        },
    )?;
    output::print_json(&out, pretty)
}

pub fn context(args: &mut Args) -> Result<()> {
    let path = PathBuf::from(next(args, "usage: model context <model.json> --key <key>")?);
    let mut key = None;
    let mut relations = Vec::new();
    let mut limit = 20;
    let mut pretty = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--key" => key = Some(next(args, "--key <key>")?),
            "--relation" => relations.extend(output::kinds(&next(args, "--relation <kind>")?)),
            "--limit" => limit = next(args, "--limit <n>")?.parse()?,
            "--pretty" => pretty = true,
            "-h" | "--help" => {
                println!(
                    "usage: model context <model.json> --key <key> [--relation <kind> ...] [--limit <n>] [--pretty]"
                );
                return Ok(());
            }
            _ => bail!("unexpected argument {a:?}"),
        }
    }
    let key = key.context("model context needs --key <key>")?;
    let model = load::model(&path)?;
    let out = api::context(&model, &key, &relations, limit)?;
    output::print_json(&out, pretty)
}

pub fn impact(args: &mut Args) -> Result<()> {
    let path = PathBuf::from(next(
        args,
        "usage: model impact <model.json> --seed <key> ...",
    )?);
    let mut seeds = Vec::new();
    let mut direction = ImpactDirection::Both;
    let mut depth = None;
    let mut kinds = Vec::new();
    let mut limit = 50;
    let mut pretty = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--seed" => seeds.push(next(args, "--seed <key>")?),
            "--direction" => {
                direction = match next(args, "--direction callers|callees|both")?.as_str() {
                    "callers" => ImpactDirection::Callers,
                    "callees" => ImpactDirection::Callees,
                    "both" => ImpactDirection::Both,
                    other => bail!("expected callers|callees|both, not {other:?}"),
                }
            }
            "--depth" => {
                let n: usize = next(args, "--depth <n>")?.parse()?;
                depth = (n > 0).then_some(n);
            }
            "--kinds" => kinds.extend(output::kinds(&next(args, "--kinds <kind,kind>")?)),
            "--limit" => limit = next(args, "--limit <n>")?.parse()?,
            "--pretty" => pretty = true,
            "-h" | "--help" => {
                println!(
                    "usage: model impact <model.json> --seed <key> ... [--direction callers|callees|both] [--depth <n>] [--kinds <kind,kind>] [--limit <n>] [--pretty]"
                );
                return Ok(());
            }
            _ => bail!("unexpected argument {a:?}"),
        }
    }
    let model = load::model(&path)?;
    let out = api::impact(&model, &seeds, direction, depth, kinds, limit)?;
    output::print_json(&out, pretty)
}

pub fn analysis(args: &mut Args) -> Result<()> {
    let path = PathBuf::from(next(args, "usage: model analysis <model.json> <name>")?);
    let mut name: Option<String> = None;
    let mut kinds: Vec<RelationKind> = Vec::new();
    let mut limit = 50;
    let mut pretty = false;
    while let Some(a) = args.next() {
        if !a.starts_with('-') && name.is_none() {
            name = Some(a);
            continue;
        }
        match a.as_str() {
            "--kinds" => kinds.extend(output::kinds(&next(args, "--kinds <kind,kind>")?)),
            "--limit" => limit = next(args, "--limit <n>")?.parse()?,
            "--pretty" => pretty = true,
            "-h" | "--help" => {
                println!(
                    "usage: model analysis <model.json> <metrics|cycles|unused|degrees> [--kinds <kind,kind>] [--limit <n>] [--pretty]"
                );
                return Ok(());
            }
            _ => bail!("unexpected argument {a:?}"),
        }
    }
    let name = name.context("model analysis needs an analysis name")?;
    let model = load::model(&path)?;
    let result = api::analysis(&model, &name, kinds, limit)?;
    let out = AnalysisOut {
        analysis: &name,
        result,
    };
    output::print_json(&out, pretty)
}
