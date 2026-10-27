//! `breakwater snapshot`: record per-handler resource usage, or check it.
//!
//! Exit codes follow `docs/ARCHITECTURE.md` §6: `0` recorded, or within the
//! tolerance; `1` a resource regressed; `2` usage or configuration error,
//! which includes a snapshot that is missing, malformed or stale, and a
//! `--tolerance` that does not parse.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context};
use breakwater_pack_budget_snapshot::{
    compare_tiered, BudgetReport, Comparison, Finding, Resource, ResourceClass, Snapshot,
    SnapshotError, SuiteSnapshot, Tolerances, Toolchain, Verdict, REPORT_ENV, SNAPSHOT_FILE,
};
use clap::Args;

use crate::driver::{self, TestRun};
use crate::style::Style;
use crate::{parse_steps, Auth};

const TEST: &str = "breakwater_snapshot";
const EXIT_OK: u8 = 0;
const EXIT_REGRESSION: u8 = 1;
const EXIT_STALE: u8 = 2;

/// Arguments of `breakwater snapshot`.
#[derive(Debug, Args)]
pub struct SnapshotArgs {
    /// Directory of the suite crate (one with a `tests/suite.rs` that has a
    /// `breakwater_snapshot` test).
    suite_dir: PathBuf,
    /// Compare with the snapshot instead of writing it. Replays the seed,
    /// step range and authorization mode the snapshot recorded.
    #[arg(long, conflicts_with_all = ["seed", "steps", "auth"])]
    check: bool,
    /// How far above the snapshot a value may be before `--check` fails:
    /// `5%` for every resource, or `storage=0%,compute=5%` per class or
    /// resource. Default 5%.
    #[arg(
        long,
        requires = "check",
        value_name = "SPEC",
        value_parser = parse_tolerances,
        long_help = tolerance_help()
    )]
    tolerance: Option<Tolerances>,
    /// Seed for deterministic sequence generation.
    #[arg(long)]
    seed: Option<u64>,
    /// Inclusive step range, e.g. `1..=20`.
    #[arg(long, value_parser = parse_steps)]
    steps: Option<String>,
    /// Authorization mode.
    #[arg(long, value_enum)]
    auth: Option<Auth>,
    /// Snapshot file.
    #[arg(long, default_value = SNAPSHOT_FILE)]
    file: PathBuf,
    /// Name of the suite entry to check. Needed only when the file holds
    /// several suites.
    #[arg(long, requires = "check")]
    suite: Option<String>,
    /// Cargo features to enable in the suite crate, for example a switch that
    /// builds the contract with a change to measure.
    #[arg(long)]
    features: Option<String>,
}

fn parse_tolerances(text: &str) -> Result<Tolerances, String> {
    Tolerances::parse(text).map_err(|e| e.to_string())
}

/// The long help of `--tolerance`. The classes are listed from
/// `Resource::class`, so the help cannot disagree with the comparison.
fn tolerance_help() -> String {
    let classes: Vec<String> = ResourceClass::ALL
        .iter()
        .map(|class| {
            let resources: Vec<&str> = class.resources().map(Resource::name).collect();
            format!("  {:<8} {}", class.name(), resources.join(", "))
        })
        .collect();
    format!(
        "How far above the snapshot a value may be before `--check` fails. The default is 5%.\n\n\
         A bare percentage applies to every resource: `--tolerance 2.5%`. KEY=PERCENTAGE sets \
         the tolerance of one class or one resource, and items are separated by commas: \
         `--tolerance storage=0%,compute=5%`, or `--tolerance 5%,storage=0%,instructions=3%`.\n\n\
         A resource beats its class, a class beats a bare percentage, and anything not set is 5%. \
         Each key may be given once, and so may a bare percentage. The option itself may be given \
         once.\n\n\
         Classes and the resources in them:\n{}\n\n\
         A key may also be the name of any single resource, such as `instructions`.",
        classes.join("\n")
    )
}

/// Runs the command and returns the process exit code.
pub fn run(style: &Style, args: SnapshotArgs) -> anyhow::Result<u8> {
    driver::validate_suite_dir(&args.suite_dir)?;
    if args.check {
        check(style, &args)
    } else {
        record(style, &args)
    }
}

fn record(style: &Style, args: &SnapshotArgs) -> anyhow::Result<u8> {
    // Do not replace a file that cannot be read back; the user decides.
    let existing = match Snapshot::read(&args.file) {
        Ok(snapshot) => Some(snapshot),
        Err(SnapshotError::Missing(_)) => None,
        Err(e) => return Err(file_error(&args.file, &e)),
    };
    let overrides = Overrides {
        seed: args.seed,
        steps: args.steps.as_deref(),
        auth: args.auth.map(Auth::as_str),
    };
    let report = measure(&args.suite_dir, args.features.as_deref(), &overrides)?;
    let toolchain = read_toolchain(&args.suite_dir)?;

    let mut snapshot = existing.unwrap_or_else(|| Snapshot::new(toolchain.clone()));
    snapshot
        .upsert(&report.suite, toolchain, report.snapshot.clone())
        .map_err(|e| file_error(&args.file, &e))?;
    snapshot
        .write(&args.file)
        .map_err(|e| file_error(&args.file, &e))?;

    print_header(style, &report.suite, &report.snapshot, None);
    print_table(style, &report.snapshot);
    println!(
        "{} {}",
        style.green("wrote"),
        style.dim(&format!(
            "{} (suite `{}`, {} handler(s))",
            args.file.display(),
            report.suite,
            report.snapshot.handlers.len()
        ))
    );
    Ok(EXIT_OK)
}

fn check(style: &Style, args: &SnapshotArgs) -> anyhow::Result<u8> {
    // Nothing is compiled or run before the baseline is known to be usable.
    let stored = Snapshot::read(&args.file).map_err(|e| file_error(&args.file, &e))?;
    let (name, baseline) = pick_suite(&stored, args.suite.as_deref())?;

    let overrides = Overrides {
        seed: Some(baseline.seed),
        steps: Some(baseline.steps.as_str()),
        auth: Some(baseline.auth.as_str()),
    };
    let report = measure(&args.suite_dir, args.features.as_deref(), &overrides)?;
    if report.suite != name {
        bail!(
            "the suite in {} is named `{}`, but the snapshot entry being checked is `{name}`; \
             pass --suite {}",
            args.suite_dir.display(),
            report.suite,
            report.suite
        );
    }
    let toolchain = read_toolchain(&args.suite_dir)?;

    let tolerances = args.tolerance.clone().unwrap_or_default();
    print_header(style, name, baseline, Some(&tolerances));
    let comparison = compare_tiered(
        &stored.toolchain,
        baseline,
        &toolchain,
        &report.snapshot,
        &tolerances,
    );
    render_comparison(style, &comparison, &args.suite_dir, &tolerances);
    Ok(exit_code(&comparison))
}

/// `0` within tolerance, `1` regression, `2` the snapshot does not describe
/// this measurement.
fn exit_code(comparison: &Comparison) -> u8 {
    match comparison {
        Comparison::Stale(_) => EXIT_STALE,
        Comparison::Compared(v) if v.passed() => EXIT_OK,
        Comparison::Compared(_) => EXIT_REGRESSION,
    }
}

fn file_error(path: &Path, error: &SnapshotError) -> anyhow::Error {
    match error {
        SnapshotError::Missing(_) => anyhow!(
            "snapshot file {} does not exist; create it with `breakwater snapshot <suite-dir>`",
            path.display()
        ),
        SnapshotError::Malformed(_) | SnapshotError::UnsupportedFormat { .. } => anyhow!(
            "{}: {error}; fix or delete the file and record it again",
            path.display()
        ),
        other => anyhow!("{other}"),
    }
}

fn pick_suite<'a>(
    snapshot: &'a Snapshot,
    requested: Option<&str>,
) -> anyhow::Result<(&'a str, &'a SuiteSnapshot)> {
    match requested {
        Some(name) => snapshot
            .suites
            .get_key_value(name)
            .map(|(k, v)| (k.as_str(), v))
            .ok_or_else(|| {
                anyhow!(
                    "the snapshot has no suite `{name}` (it has: {})",
                    snapshot
                        .suites
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }),
        None => {
            let mut suites = snapshot.suites.iter();
            match (suites.next(), suites.next()) {
                (Some((k, v)), None) => Ok((k.as_str(), v)),
                (None, _) => bail!("the snapshot holds no suites; record one first"),
                _ => bail!(
                    "the snapshot holds several suites ({}); choose one with --suite",
                    snapshot
                        .suites
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }
        }
    }
}

struct Overrides<'a> {
    seed: Option<u64>,
    steps: Option<&'a str>,
    auth: Option<&'a str>,
}

/// Runs the suite's `breakwater_snapshot` test and reads its measurement.
fn measure(
    dir: &Path,
    features: Option<&str>,
    overrides: &Overrides<'_>,
) -> anyhow::Result<BudgetReport> {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "suite".to_owned());
    let out = std::path::absolute(std::env::temp_dir().join(format!(
        "breakwater-budget-{name}-{}.json",
        std::process::id()
    )))
    .context("resolving the measurement path")?;
    let _ = std::fs::remove_file(&out);

    let mut env: Vec<(&str, std::ffi::OsString)> = vec![(REPORT_ENV, out.clone().into_os_string())];
    if let Some(seed) = overrides.seed {
        env.push(("BREAKWATER_SEED", seed.to_string().into()));
    }
    if let Some(steps) = overrides.steps {
        env.push(("BREAKWATER_STEPS", steps.into()));
    }
    if let Some(auth) = overrides.auth {
        env.push(("BREAKWATER_AUTH", auth.into()));
    }
    let status = driver::run_test(
        dir,
        &TestRun {
            test: TEST,
            features,
            env,
            nocapture: true,
        },
    )?;
    let report = BudgetReport::read(&out).map_err(|_| {
        anyhow!(
            "the suite wrote no measurement; cargo test exited with {status}. \
             {} needs a `{TEST}` test in tests/suite.rs that calls BudgetReport::write_if_requested",
            dir.display()
        )
    });
    let _ = std::fs::remove_file(&out);
    report
}

/// The resolved versions the numbers depend on, from `cargo metadata`.
fn read_toolchain(dir: &Path) -> anyhow::Result<Toolchain> {
    let output =
        std::process::Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .args([
                "metadata",
                "--format-version",
                "1",
                "--locked",
                "--manifest-path",
            ])
            .arg(dir.join("Cargo.toml"))
            .stdin(std::process::Stdio::null())
            .output()
            .context("running cargo metadata")?;
    if !output.status.success() {
        bail!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("parsing cargo metadata")?;
    toolchain_from_metadata(&json)
}

fn toolchain_from_metadata(metadata: &serde_json::Value) -> anyhow::Result<Toolchain> {
    let version_of = |name: &str| -> anyhow::Result<Option<String>> {
        let mut versions: Vec<&str> = metadata["packages"]
            .as_array()
            .map(|packages| {
                packages
                    .iter()
                    .filter(|p| p["name"] == name)
                    .filter_map(|p| p["version"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        versions.sort_unstable();
        versions.dedup();
        match versions.as_slice() {
            [] => Ok(None),
            [one] => Ok(Some((*one).to_owned())),
            many => bail!("{name} resolves to several versions ({})", many.join(", ")),
        }
    };
    let required = |name: &str| {
        version_of(name)?.ok_or_else(|| anyhow!("{name} is not in the suite's dependency graph"))
    };
    // The core library is published as `soroban-breakwater`; in the
    // workspace it may still be called `breakwater`.
    let breakwater = match version_of("soroban-breakwater")? {
        Some(v) => v,
        None => required("breakwater")?,
    };
    Ok(Toolchain {
        soroban_sdk: required("soroban-sdk")?,
        soroban_env_host: required("soroban-env-host")?,
        breakwater,
    })
}

fn print_header(style: &Style, name: &str, suite: &SuiteSnapshot, tolerances: Option<&Tolerances>) {
    println!(
        "{} {}   {} {}   {} {}   {} {}   {} {}",
        style.bold("suite:"),
        style.cyan(name),
        style.bold("seed:"),
        suite.seed,
        style.bold("cases:"),
        suite.cases,
        style.bold("steps:"),
        suite.steps,
        style.bold("auth:"),
        suite.auth,
    );
    let mode = format!("mode: {}", suite.mode.as_str());
    match tolerances {
        Some(t) => println!("{}", style.dim(&format!("{mode}   tolerance: {t}"))),
        None => println!("{}", style.dim(&mode)),
    }
}

/// The columns shown after recording; the file has all twelve resources.
const SHOWN: [Resource; 5] = [
    Resource::Instructions,
    Resource::DiskReadEntries,
    Resource::WriteEntries,
    Resource::WriteBytes,
    Resource::PersistentEntryRentBumps,
];

fn print_table(style: &Style, suite: &SuiteSnapshot) {
    let mut head = format!("  {:<16} {:>7}", "handler", "samples");
    for resource in SHOWN {
        head.push_str(&format!(" {:>24}", format!("{} max/p50", resource.name())));
    }
    println!("{}", style.dim(&head));
    for (name, stats) in &suite.handlers {
        let mut line = format!("  {name:<16} {:>7}", stats.samples);
        for resource in SHOWN {
            let cell = stats
                .get(resource)
                .map(|s| format!("{}/{}", s.max, s.p50))
                .unwrap_or_default();
            line.push_str(&format!(" {cell:>24}"));
        }
        println!("{line}");
    }
}

fn render_comparison(style: &Style, comparison: &Comparison, dir: &Path, tolerances: &Tolerances) {
    match comparison {
        Comparison::Stale(reasons) => {
            println!("{}", style.dim(&"─".repeat(72)));
            println!(
                "{}",
                style.red(&style.bold("the snapshot does not describe this run:"))
            );
            for reason in reasons {
                println!("  - {reason}");
            }
            println!(
                "{}",
                style.dim(&format!(
                    "record it again with `breakwater snapshot {}` if the change is intended",
                    dir.display()
                ))
            );
        }
        Comparison::Compared(verdict) => render_verdict(style, verdict, dir, tolerances),
    }
}

/// One finding, ending with the tolerance it was judged by and where that
/// tolerance came from: `limit 0% (storage)`.
fn finding_line(style: &Style, finding: &Finding, mark: &str, tolerances: &Tolerances) -> String {
    let (tolerance, source) = tolerances.resolve(finding.resource);
    let limit = format!("limit {tolerance} ({})", source.label(finding.resource));
    format!(
        "  {:<16} {:<28} {:<3} {:>12} -> {:<12} {:>8}  {mark}  {}",
        finding.handler,
        finding.resource.name(),
        finding.stat.name(),
        finding.baseline,
        finding.current,
        style.bold(&finding.delta_text()),
        style.dim(&limit),
    )
}

/// `storage 10, compute 4`: how many of `findings` are in each class.
fn class_counts<'a>(findings: impl Iterator<Item = &'a Finding> + Clone) -> String {
    ResourceClass::ALL
        .iter()
        .map(|class| {
            let n = findings.clone().filter(|f| f.class() == *class).count();
            format!("{} {n}", class.name())
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn summary_line(verdict: &Verdict) -> String {
    let mut handlers: Vec<&str> = verdict
        .regressions
        .iter()
        .map(|f| f.handler.as_str())
        .chain(verdict.sample_changes.iter().map(|c| c.handler.as_str()))
        .collect();
    handlers.sort_unstable();
    handlers.dedup();
    format!(
        "{} value(s) over the tolerance ({}) and {} sample count change(s) in {} handler(s)",
        verdict.regressions.len(),
        class_counts(verdict.regressions.iter()),
        verdict.sample_changes.len(),
        handlers.len()
    )
}

fn render_verdict(style: &Style, verdict: &Verdict, dir: &Path, tolerances: &Tolerances) {
    println!("{}", style.dim(&"─".repeat(72)));
    for finding in &verdict.regressions {
        let mark = style.red("REGRESSION");
        println!("{}", finding_line(style, finding, &mark, tolerances));
    }
    for change in &verdict.sample_changes {
        println!(
            "  {:<16} {:<28} {:>16} -> {:<12}  {}",
            change.handler,
            "samples",
            change.baseline,
            change.current,
            style.red("CHANGED (the same workload now measures a different number of calls)")
        );
    }
    for finding in &verdict.improvements {
        let mark = style.green("improved");
        println!("{}", finding_line(style, finding, &mark, tolerances));
    }
    if verdict.passed() {
        println!("{}", style.green("all handlers are within the tolerance"));
        if !verdict.improvements.is_empty() {
            println!(
                "{}",
                style.dim(&format!(
                    "{} value(s) improved ({}); run `breakwater snapshot {}` to record the lower baseline",
                    verdict.improvements.len(),
                    class_counts(verdict.improvements.iter()),
                    dir.display()
                ))
            );
        }
    } else {
        println!("{}", style.red(&summary_line(verdict)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use breakwater_pack_budget_snapshot::{Mode, SampleChange, StatKind};
    use clap::Parser;
    use std::collections::BTreeMap;

    /// `SnapshotArgs` is a subcommand's arguments; this makes it parseable alone.
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: SnapshotArgs,
    }

    fn parse(args: &[&str]) -> Result<SnapshotArgs, clap::Error> {
        TestCli::try_parse_from(
            ["snapshot", "suite"]
                .into_iter()
                .chain(args.iter().copied()),
        )
        .map(|cli| cli.args)
    }

    fn finding(resource: Resource, baseline: i64, current: i64) -> Finding {
        Finding {
            handler: "withdraw".into(),
            resource,
            stat: StatKind::Max,
            baseline,
            current,
        }
    }

    fn suite() -> SuiteSnapshot {
        SuiteSnapshot {
            seed: 7,
            cases: 4,
            steps: "1..=5".into(),
            auth: "mock".into(),
            mode: Mode::Native,
            workload_digest: "fnv1a64:0000000000000000".into(),
            handlers: BTreeMap::new(),
        }
    }

    fn snapshot(names: &[&str]) -> Snapshot {
        let mut s = Snapshot::new(Toolchain {
            soroban_sdk: "28.0.0".into(),
            soroban_env_host: "28.0.2".into(),
            breakwater: "0.1.0".into(),
        });
        for name in names {
            s.suites.insert((*name).to_owned(), suite());
        }
        s
    }

    #[test]
    fn exit_codes_follow_the_run_command() {
        assert_eq!(exit_code(&Comparison::Stale(vec!["x".into()])), 2);
        assert_eq!(exit_code(&Comparison::Compared(Verdict::default())), 0);
        let mut changed = Verdict::default();
        changed.sample_changes.push(SampleChange {
            handler: "h".into(),
            baseline: 2,
            current: 1,
        });
        assert_eq!(exit_code(&Comparison::Compared(changed)), 1);
    }

    #[test]
    fn picks_the_only_suite_or_the_named_one() {
        let one = snapshot(&["a"]);
        assert_eq!(pick_suite(&one, None).unwrap().0, "a");
        let two = snapshot(&["a", "b"]);
        assert!(pick_suite(&two, None).is_err());
        assert_eq!(pick_suite(&two, Some("b")).unwrap().0, "b");
        assert!(pick_suite(&two, Some("c")).is_err());
        assert!(pick_suite(&snapshot(&[]), None).is_err());
    }

    #[test]
    fn reads_the_toolchain_from_cargo_metadata() {
        let metadata = serde_json::json!({ "packages": [
            { "name": "soroban-sdk", "version": "28.0.0" },
            { "name": "soroban-env-host", "version": "28.0.2" },
            { "name": "breakwater", "version": "0.1.0" },
        ]});
        let t = toolchain_from_metadata(&metadata).unwrap();
        assert_eq!(
            (
                t.soroban_sdk.as_str(),
                t.soroban_env_host.as_str(),
                t.breakwater.as_str()
            ),
            ("28.0.0", "28.0.2", "0.1.0")
        );

        // After the crates.io rename the core library is `soroban-breakwater`.
        let renamed = serde_json::json!({ "packages": [
            { "name": "soroban-sdk", "version": "28.0.0" },
            { "name": "soroban-env-host", "version": "28.0.2" },
            { "name": "soroban-breakwater", "version": "0.2.0" },
        ]});
        assert_eq!(
            toolchain_from_metadata(&renamed).unwrap().breakwater,
            "0.2.0"
        );

        let missing = serde_json::json!({ "packages": [
            { "name": "soroban-sdk", "version": "28.0.0" },
        ]});
        assert!(toolchain_from_metadata(&missing).is_err());
        let ambiguous = serde_json::json!({ "packages": [
            { "name": "soroban-sdk", "version": "28.0.0" },
            { "name": "soroban-sdk", "version": "27.0.0" },
            { "name": "soroban-env-host", "version": "28.0.2" },
            { "name": "breakwater", "version": "0.1.0" },
        ]});
        assert!(toolchain_from_metadata(&ambiguous).is_err());
    }

    #[test]
    fn tolerance_takes_a_bare_value_or_a_list() {
        let tolerance = |args: &[&str]| parse(args).expect("parses").tolerance;
        assert_eq!(tolerance(&["--check"]), None);
        assert_eq!(
            tolerance(&["--check", "--tolerance", "0%"]),
            Some(Tolerances::parse("0%").unwrap())
        );
        let tiered = tolerance(&["--check", "--tolerance", "storage=0%,compute=5%"]).unwrap();
        assert_eq!(tiered.to_string(), "storage 0%, compute 5%");
        assert_eq!(tiered.for_resource(Resource::WriteBytes).basis_points(), 0);
        assert_eq!(tiered.for_resource(Resource::MemBytes).basis_points(), 500);
        // `--tolerance=...` and a single resource work too.
        let one = tolerance(&["--check", "--tolerance=instructions=3%"]).unwrap();
        assert_eq!(one.for_resource(Resource::Instructions).basis_points(), 300);
        assert_eq!(one.for_resource(Resource::MemBytes).basis_points(), 500);
    }

    #[test]
    fn a_bad_tolerance_is_a_usage_error_with_exit_code_2() {
        let usage = |args: &[&str]| {
            let err = parse(args).expect_err(&format!("{args:?} was accepted"));
            assert_eq!(err.exit_code(), 2, "{args:?}");
            err.to_string()
        };
        for spec in [
            "lots",
            "",
            ",",
            "5%,",
            "cpu=5%",
            "Storage=0%",
            "storage=",
            "=5%",
            "storage=lots",
            "storage=0%,storage=1%",
            "instructions=1%,instructions=2%",
            "5%,5%",
            "storage=42949673%",
        ] {
            usage(&["--check", "--tolerance", spec]);
        }
        // The message says what is wrong and which keys would work.
        let message = usage(&["--check", "--tolerance", "cpu=5%"]);
        assert!(message.contains("unknown tolerance key `cpu`"), "{message}");
        assert!(
            message.contains("storage, compute, instructions"),
            "{message}"
        );
        // The option may be given once, and only together with --check.
        usage(&["--check", "--tolerance", "0%", "--tolerance", "5%"]);
        usage(&[
            "--check",
            "--tolerance",
            "storage=0%",
            "--tolerance",
            "compute=5%",
        ]);
        usage(&["--tolerance", "0%"]);
        usage(&["--tolerance", "storage=0%,compute=5%"]);
    }

    #[test]
    fn the_help_lists_the_classes_as_the_comparison_sees_them() {
        let help = tolerance_help();
        for class in ResourceClass::ALL {
            let line = help
                .lines()
                .find(|l| l.trim_start().starts_with(class.name()) && l.contains(','))
                .unwrap_or_else(|| panic!("no line for {}: {help}", class.name()));
            for resource in Resource::ALL {
                assert_eq!(
                    line.contains(resource.name()),
                    resource.class() == *class,
                    "{} in `{line}`",
                    resource.name()
                );
            }
        }
        assert!(help.contains("storage=0%,compute=5%"));
    }

    #[test]
    fn a_line_names_the_tolerance_it_was_judged_by() {
        let style = Style::detect(true);
        let tolerances = Tolerances::parse("storage=0%,compute=5%,mem_bytes=3%").unwrap();
        let line = |resource, from, to| {
            finding_line(
                &style,
                &finding(resource, from, to),
                "REGRESSION",
                &tolerances,
            )
        };
        let storage = line(Resource::WriteEntries, 4, 5);
        assert!(storage.contains("write_entries"), "{storage}");
        assert!(
            storage.contains("+25.0%  REGRESSION  limit 0% (storage)"),
            "{storage}"
        );
        let compute = line(Resource::Instructions, 1000, 1200);
        assert!(
            compute.ends_with("REGRESSION  limit 5% (compute)"),
            "{compute}"
        );
        let single = line(Resource::MemBytes, 1000, 1100);
        assert!(
            single.ends_with("REGRESSION  limit 3% (mem_bytes)"),
            "{single}"
        );

        let uniform = Tolerances::parse("2.5%").unwrap();
        let all = finding_line(
            &style,
            &finding(Resource::WriteBytes, 100, 110),
            "REGRESSION",
            &uniform,
        );
        assert!(all.ends_with("limit 2.5% (all)"), "{all}");
        let default = finding_line(
            &style,
            &finding(Resource::WriteBytes, 100, 110),
            "improved",
            &Tolerances::default(),
        );
        assert!(
            default.ends_with("improved  limit 5% (default)"),
            "{default}"
        );
    }

    #[test]
    fn the_summary_counts_each_class() {
        let mut verdict = Verdict::default();
        for resource in [
            Resource::WriteEntries,
            Resource::WriteBytes,
            Resource::Instructions,
        ] {
            verdict.regressions.push(finding(resource, 1, 2));
        }
        verdict.regressions.push(Finding {
            handler: "cancel".into(),
            ..finding(Resource::MemBytes, 1, 2)
        });
        verdict.sample_changes.push(SampleChange {
            handler: "create_stream".into(),
            baseline: 2,
            current: 1,
        });
        assert_eq!(
            summary_line(&verdict),
            "4 value(s) over the tolerance (storage 2, compute 2) and 1 sample count change(s) \
             in 3 handler(s)"
        );
        // A class without findings is still listed, with a zero.
        let mut storage_only = Verdict::default();
        storage_only
            .regressions
            .push(finding(Resource::WriteBytes, 1, 2));
        assert_eq!(
            summary_line(&storage_only),
            "1 value(s) over the tolerance (storage 1, compute 0) and 0 sample count change(s) \
             in 1 handler(s)"
        );
    }
}
