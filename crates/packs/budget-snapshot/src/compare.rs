//! Comparing a fresh measurement with a stored snapshot.
//!
//! The comparison has three outcomes. If the two cannot be compared at all
//! (other toolchain, other workload, other handlers) the snapshot is
//! [`Comparison::Stale`]. Otherwise every handler's `max` and `p50` per
//! resource is checked against the baseline within a [`Tolerance`], and the
//! sample counts are compared: with the same generated workload, fewer or more
//! successful calls means the contract's behaviour changed, which would let a
//! handler quietly drop out of the measurement.
//!
//! [`compare`] applies one tolerance to every resource. [`compare_tiered`]
//! takes a [`Tolerances`] instead, which can give the storage figures and
//! `instructions` / `mem_bytes` (see [`ResourceClass`]) different tolerances,
//! or single resources their own.

use std::collections::BTreeMap;
use std::fmt;

use crate::resource::{Resource, ResourceClass};
use crate::snapshot::{SuiteSnapshot, Toolchain};

/// How far above the baseline a value may be and still pass, in basis points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tolerance {
    bps: u32,
}

/// A tolerance that could not be parsed.
#[derive(Debug, thiserror::Error)]
#[error("invalid tolerance `{0}`: expected a non-negative percentage like 5%, 2.5% or 0%")]
pub struct ToleranceError(String);

impl Default for Tolerance {
    fn default() -> Self {
        Tolerance { bps: 500 }
    }
}

impl Tolerance {
    /// Parses `5%`, `2.5%`, `0.25%` or a bare `5`. At most two decimals.
    pub fn parse(text: &str) -> Result<Tolerance, ToleranceError> {
        let err = || ToleranceError(text.to_owned());
        let body = text.trim().strip_suffix('%').unwrap_or(text.trim()).trim();
        let (whole, frac) = body.split_once('.').unwrap_or((body, ""));
        let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
        if whole.is_empty() || !digits(whole) || !digits(frac) || frac.len() > 2 {
            return Err(err());
        }
        let whole: u32 = whole.parse().map_err(|_| err())?;
        let frac: u32 = if frac.is_empty() {
            0
        } else {
            format!("{frac:0<2}").parse().map_err(|_| err())?
        };
        let bps = whole
            .checked_mul(100)
            .and_then(|b| b.checked_add(frac))
            .ok_or_else(err)?;
        Ok(Tolerance { bps })
    }

    /// The tolerance in basis points (hundredths of a percent).
    pub fn basis_points(self) -> u32 {
        self.bps
    }

    /// True when `current` is at most `baseline` plus the tolerance. Integer
    /// arithmetic only; a baseline of zero allows only zero.
    pub fn allows(self, baseline: i64, current: i64) -> bool {
        i128::from(current) * 10_000 <= i128::from(baseline) * (10_000 + i128::from(self.bps))
    }

    /// True when `current` is below `baseline` by more than the tolerance.
    fn improves(self, baseline: i64, current: i64) -> bool {
        i128::from(current) * 10_000 < i128::from(baseline) * (10_000 - i128::from(self.bps))
    }
}

impl fmt::Display for Tolerance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (whole, frac) = (self.bps / 100, self.bps % 100);
        match frac {
            0 => write!(f, "{whole}%"),
            f2 if f2 % 10 == 0 => write!(f, "{whole}.{}%", f2 / 10),
            f2 => write!(f, "{whole}.{f2:02}%"),
        }
    }
}

/// Where the tolerance that applies to a resource came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToleranceSource {
    /// Set for this resource by name.
    Resource,
    /// Set for the resource's class.
    Class(ResourceClass),
    /// A bare value that applies to every resource.
    All,
    /// Nothing was set: the default of [`Tolerance::default`].
    Default,
}

impl ToleranceSource {
    /// A short label for reports: the resource name, the class name, `all` or
    /// `default`.
    pub fn label(self, resource: Resource) -> &'static str {
        match self {
            ToleranceSource::Resource => resource.name(),
            ToleranceSource::Class(class) => class.name(),
            ToleranceSource::All => "all",
            ToleranceSource::Default => "default",
        }
    }
}

/// The tolerance of every resource.
///
/// A resource's tolerance is the first that is set of: the one for the
/// resource itself, the one for its [`ResourceClass`], the one for all
/// resources, and [`Tolerance::default`]. [`Tolerances::uniform`] is the
/// single tolerance that [`compare`] takes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tolerances {
    all: Option<Tolerance>,
    classes: BTreeMap<ResourceClass, Tolerance>,
    resources: BTreeMap<Resource, Tolerance>,
}

/// A tolerance list that could not be parsed, see [`Tolerances::parse`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TolerancesError {
    /// An empty item, such as a leading, trailing or doubled comma.
    #[error("empty item in the tolerance list; write items like `5%` or `storage=0%`, separated by commas")]
    EmptyItem,
    /// A `=` with nothing before it.
    #[error("missing key before `=`; keys are: {keys}", keys = Tolerances::keys())]
    MissingKey,
    /// A key that is neither a class nor a resource.
    #[error("unknown tolerance key `{0}`; keys are: {keys}", keys = Tolerances::keys())]
    UnknownKey(String),
    /// A value that is not a percentage.
    #[error("{}", value_message(.key, .source))]
    Value {
        /// The key the value was given for; `None` for a bare value.
        key: Option<String>,
        /// Why the value was rejected.
        source: ToleranceError,
    },
    /// The same class or resource twice.
    #[error("the tolerance for `{0}` is given more than once")]
    DuplicateKey(String),
    /// More than one bare value.
    #[error("more than one tolerance without a key; give at most one")]
    DuplicateAll,
}

fn value_message(key: &Option<String>, source: &ToleranceError) -> String {
    match key {
        Some(key) => format!("tolerance for `{key}`: {source}"),
        None => format!(
            "{source}; to set one class or resource write KEY=PERCENTAGE, with KEY one of: {}",
            Tolerances::keys()
        ),
    }
}

impl Tolerances {
    /// `tolerance` for every resource.
    pub fn uniform(tolerance: Tolerance) -> Tolerances {
        Tolerances {
            all: Some(tolerance),
            ..Tolerances::default()
        }
    }

    /// Sets the tolerance of every resource of `class`, replacing an earlier
    /// one for the same class.
    #[must_use]
    pub fn with_class(mut self, class: ResourceClass, tolerance: Tolerance) -> Tolerances {
        self.classes.insert(class, tolerance);
        self
    }

    /// Sets the tolerance of one resource, replacing an earlier one for the
    /// same resource.
    #[must_use]
    pub fn with_resource(mut self, resource: Resource, tolerance: Tolerance) -> Tolerances {
        self.resources.insert(resource, tolerance);
        self
    }

    /// The tolerance that applies to `resource` and where it came from.
    pub fn resolve(&self, resource: Resource) -> (Tolerance, ToleranceSource) {
        match self.resources.get(&resource) {
            Some(t) => (*t, ToleranceSource::Resource),
            None => self.resolve_class(resource.class()),
        }
    }

    /// The tolerance that applies to `resource`.
    pub fn for_resource(&self, resource: Resource) -> Tolerance {
        self.resolve(resource).0
    }

    fn resolve_class(&self, class: ResourceClass) -> (Tolerance, ToleranceSource) {
        match (self.classes.get(&class), self.all) {
            (Some(t), _) => (*t, ToleranceSource::Class(class)),
            (None, Some(t)) => (t, ToleranceSource::All),
            (None, None) => (Tolerance::default(), ToleranceSource::Default),
        }
    }

    /// Every key [`Tolerances::parse`] accepts: the class names, then the
    /// resource names.
    fn keys() -> String {
        let classes = ResourceClass::ALL.iter().map(|c| c.name());
        let resources = Resource::ALL.iter().map(|r| r.name());
        classes.chain(resources).collect::<Vec<_>>().join(", ")
    }

    /// Parses a comma-separated list of items: a bare percentage that applies
    /// to every resource (at most one), and `KEY=PERCENTAGE` where the key is
    /// a class (`storage`, `compute`) or a resource name such as
    /// `instructions`. Percentages are those of [`Tolerance::parse`]. Every
    /// key may appear once. A single bare percentage is the same as
    /// [`Tolerances::uniform`].
    pub fn parse(spec: &str) -> Result<Tolerances, TolerancesError> {
        let mut out = Tolerances::default();
        for item in spec.split(',') {
            let item = item.trim();
            if item.is_empty() {
                return Err(TolerancesError::EmptyItem);
            }
            let Some((key, value)) = item.split_once('=') else {
                let tolerance = Tolerance::parse(item)
                    .map_err(|source| TolerancesError::Value { key: None, source })?;
                if out.all.replace(tolerance).is_some() {
                    return Err(TolerancesError::DuplicateAll);
                }
                continue;
            };
            let key = key.trim();
            if key.is_empty() {
                return Err(TolerancesError::MissingKey);
            }
            let class = ResourceClass::from_name(key);
            let resource = Resource::from_name(key);
            if class.is_none() && resource.is_none() {
                return Err(TolerancesError::UnknownKey(key.to_owned()));
            }
            let tolerance = Tolerance::parse(value).map_err(|source| TolerancesError::Value {
                key: Some(key.to_owned()),
                source,
            })?;
            let duplicate = match (class, resource) {
                (Some(class), _) => out.classes.insert(class, tolerance).is_some(),
                (None, Some(resource)) => out.resources.insert(resource, tolerance).is_some(),
                (None, None) => false,
            };
            if duplicate {
                return Err(TolerancesError::DuplicateKey(key.to_owned()));
            }
        }
        Ok(out)
    }
}

/// `5%` when one value covers everything, as before. Otherwise one entry per
/// class, then the resources that were set by name: `storage 0%, compute 5%`.
impl fmt::Display for Tolerances {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.classes.is_empty() && self.resources.is_empty() {
            return write!(f, "{}", self.all.unwrap_or_default());
        }
        let mut parts = Vec::new();
        for class in ResourceClass::ALL {
            let (tolerance, source) = self.resolve_class(*class);
            let note = if source == ToleranceSource::Default {
                " (default)"
            } else {
                ""
            };
            parts.push(format!("{} {tolerance}{note}", class.name()));
        }
        for (resource, tolerance) in &self.resources {
            parts.push(format!("{} {tolerance}", resource.name()));
        }
        write!(f, "{}", parts.join(", "))
    }
}

/// Which statistic a finding is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatKind {
    /// The largest sample.
    Max,
    /// The lower median.
    P50,
}

impl StatKind {
    /// The key used in the file.
    pub fn name(self) -> &'static str {
        match self {
            StatKind::Max => "max",
            StatKind::P50 => "p50",
        }
    }
}

/// One resource of one handler that moved by more than the tolerance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// Handler name.
    pub handler: String,
    /// The resource.
    pub resource: Resource,
    /// The statistic.
    pub stat: StatKind,
    /// Value in the snapshot.
    pub baseline: i64,
    /// Value now.
    pub current: i64,
}

impl Finding {
    /// The class of the resource that moved.
    pub fn class(&self) -> ResourceClass {
        self.resource.class()
    }

    /// The change relative to the baseline, like `+33.3%`; `new` when the
    /// baseline is zero.
    pub fn delta_text(&self) -> String {
        if self.baseline == 0 {
            return "new".to_owned();
        }
        let tenths = i128::from(self.current - self.baseline) * 1000 / i128::from(self.baseline);
        let sign = if tenths < 0 { "-" } else { "+" };
        format!("{sign}{}.{}%", tenths.abs() / 10, tenths.abs() % 10)
    }
}

/// A handler whose number of measured calls changed under the same workload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleChange {
    /// Handler name.
    pub handler: String,
    /// Samples in the snapshot.
    pub baseline: u64,
    /// Samples now.
    pub current: u64,
}

/// The result of comparing two measurements that are comparable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Verdict {
    /// Values above the baseline by more than the tolerance.
    pub regressions: Vec<Finding>,
    /// Values below the baseline by more than the tolerance. Informational.
    pub improvements: Vec<Finding>,
    /// Handlers whose sample count changed.
    pub sample_changes: Vec<SampleChange>,
}

impl Verdict {
    /// True when nothing regressed and no handler changed its sample count.
    pub fn passed(&self) -> bool {
        self.regressions.is_empty() && self.sample_changes.is_empty()
    }

    /// The regressions in resources of `class`.
    pub fn regressions_in(&self, class: ResourceClass) -> impl Iterator<Item = &Finding> {
        self.regressions.iter().filter(move |f| f.class() == class)
    }

    /// The improvements in resources of `class`.
    pub fn improvements_in(&self, class: ResourceClass) -> impl Iterator<Item = &Finding> {
        self.improvements.iter().filter(move |f| f.class() == class)
    }
}

/// Whether a snapshot can be compared with a measurement at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Comparison {
    /// The snapshot does not describe this measurement; each entry says why.
    Stale(Vec<String>),
    /// Both describe the same workload; here is how the numbers moved.
    Compared(Verdict),
}

/// Compares `current` with `baseline`, within one `tolerance` for every
/// resource.
///
/// The toolchains differ when `soroban-sdk` or `soroban-env-host` differ; the
/// core library version is recorded for the reader but does not by itself
/// change a number, and a change to what it generates shows in the workload
/// digest.
pub fn compare(
    baseline_toolchain: &Toolchain,
    baseline: &SuiteSnapshot,
    current_toolchain: &Toolchain,
    current: &SuiteSnapshot,
    tolerance: Tolerance,
) -> Comparison {
    compare_tiered(
        baseline_toolchain,
        baseline,
        current_toolchain,
        current,
        &Tolerances::uniform(tolerance),
    )
}

/// Compares `current` with `baseline`, checking every resource against the
/// tolerance that `tolerances` gives it. Improvements are judged by the same
/// tolerance as regressions. Otherwise this is [`compare`].
pub fn compare_tiered(
    baseline_toolchain: &Toolchain,
    baseline: &SuiteSnapshot,
    current_toolchain: &Toolchain,
    current: &SuiteSnapshot,
    tolerances: &Tolerances,
) -> Comparison {
    let mut stale = Vec::new();
    let mut differs = |what: &str, was: &dyn fmt::Display, now: &dyn fmt::Display| {
        stale.push(format!("{what} was {was} in the snapshot, now {now}"));
    };
    if baseline_toolchain.soroban_sdk != current_toolchain.soroban_sdk {
        differs(
            "soroban-sdk",
            &baseline_toolchain.soroban_sdk,
            &current_toolchain.soroban_sdk,
        );
    }
    if baseline_toolchain.soroban_env_host != current_toolchain.soroban_env_host {
        differs(
            "soroban-env-host",
            &baseline_toolchain.soroban_env_host,
            &current_toolchain.soroban_env_host,
        );
    }
    if baseline.mode != current.mode {
        differs("mode", &baseline.mode.as_str(), &current.mode.as_str());
    }
    if baseline.auth != current.auth {
        differs("auth", &baseline.auth, &current.auth);
    }
    if baseline.seed != current.seed {
        differs("seed", &baseline.seed, &current.seed);
    }
    if baseline.cases != current.cases {
        differs("cases", &baseline.cases, &current.cases);
    }
    if baseline.steps != current.steps {
        differs("steps", &baseline.steps, &current.steps);
    }
    if baseline.workload_digest != current.workload_digest {
        differs(
            "the generated workload (digest)",
            &baseline.workload_digest,
            &current.workload_digest,
        );
    }
    for name in baseline.handlers.keys() {
        if !current.handlers.contains_key(name) {
            stale.push(format!(
                "handler `{name}` is in the snapshot but was not measured"
            ));
        }
    }
    for name in current.handlers.keys() {
        if !baseline.handlers.contains_key(name) {
            stale.push(format!(
                "handler `{name}` was measured but is not in the snapshot"
            ));
        }
    }
    if !stale.is_empty() {
        return Comparison::Stale(stale);
    }

    let mut verdict = Verdict::default();
    for (handler, base) in &baseline.handlers {
        let Some(now) = current.handlers.get(handler) else {
            continue;
        };
        if base.samples != now.samples {
            verdict.sample_changes.push(SampleChange {
                handler: handler.clone(),
                baseline: base.samples,
                current: now.samples,
            });
        }
        for resource in Resource::ALL {
            let (Some(b), Some(c)) = (base.get(resource), now.get(resource)) else {
                continue;
            };
            let tolerance = tolerances.for_resource(resource);
            for (stat, was, is) in [(StatKind::Max, b.max, c.max), (StatKind::P50, b.p50, c.p50)] {
                let finding = || Finding {
                    handler: handler.clone(),
                    resource,
                    stat,
                    baseline: was,
                    current: is,
                };
                if !tolerance.allows(was, is) {
                    verdict.regressions.push(finding());
                } else if tolerance.improves(was, is) {
                    verdict.improvements.push(finding());
                }
            }
        }
    }
    Comparison::Compared(verdict)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::resource::Sample;
    use crate::snapshot::Mode;
    use crate::stats::HandlerStats;

    fn toolchain() -> Toolchain {
        Toolchain {
            soroban_sdk: "28.0.0".into(),
            soroban_env_host: "28.0.2".into(),
            breakwater: "0.1.0".into(),
        }
    }

    fn suite(handlers: &[(&str, i64, i64, u64)]) -> SuiteSnapshot {
        let handlers: BTreeMap<String, HandlerStats> = handlers
            .iter()
            .map(|(name, instructions, writes, n)| {
                let sample = Sample {
                    instructions: *instructions,
                    write_entries: *writes,
                    ..Sample::default()
                };
                let mut stats = HandlerStats::from_samples(&[sample]).expect("sample");
                stats.samples = *n;
                ((*name).to_owned(), stats)
            })
            .collect();
        SuiteSnapshot {
            seed: 7,
            cases: 16,
            steps: "1..=20".into(),
            auth: "mock".into(),
            mode: Mode::Native,
            workload_digest: "fnv1a64:00000000000000aa".into(),
            handlers,
        }
    }

    fn run(baseline: &SuiteSnapshot, current: &SuiteSnapshot, tol: &str) -> Comparison {
        compare(
            &toolchain(),
            baseline,
            &toolchain(),
            current,
            Tolerance::parse(tol).expect("tolerance"),
        )
    }

    fn verdict(c: Comparison) -> Verdict {
        match c {
            Comparison::Compared(v) => v,
            Comparison::Stale(why) => panic!("stale: {why:?}"),
        }
    }

    #[test]
    fn parses_tolerances() {
        for (text, bps) in [
            ("5%", 500),
            (" 5 % ", 500),
            ("5", 500),
            ("2.5%", 250),
            ("0.25%", 25),
            ("0%", 0),
            ("100%", 10_000),
        ] {
            assert_eq!(
                Tolerance::parse(text).expect(text).basis_points(),
                bps,
                "{text}"
            );
        }
        for text in [
            "", "%", "-1%", "abc", "5.%x", "1.234%", ".5%", "5%%", "1e2%", "+5%",
        ] {
            assert!(Tolerance::parse(text).is_err(), "{text} was accepted");
        }
        assert_eq!(Tolerance::default().to_string(), "5%");
        assert_eq!(Tolerance::parse("2.5").unwrap().to_string(), "2.5%");
        assert_eq!(Tolerance::parse("0.25").unwrap().to_string(), "0.25%");
    }

    #[test]
    fn the_boundary_passes_and_one_more_fails() {
        let five = Tolerance::parse("5%").unwrap();
        assert!(five.allows(100, 105));
        assert!(!five.allows(100, 106));
        assert!(five.allows(100, 0));
        let zero = Tolerance::parse("0%").unwrap();
        assert!(zero.allows(100, 100));
        assert!(!zero.allows(100, 101));
        // A baseline of zero allows only zero, whatever the tolerance.
        assert!(five.allows(0, 0));
        assert!(!five.allows(0, 1));
        // No overflow at the top of the range.
        assert!(zero.allows(i64::MAX, i64::MAX));
    }

    #[test]
    fn identical_measurements_pass() {
        let s = suite(&[("withdraw", 1000, 2, 5)]);
        let v = verdict(run(&s, &s, "5%"));
        assert!(v.passed());
        assert!(v.improvements.is_empty());
    }

    #[test]
    fn a_regression_names_the_handler_resource_and_statistic() {
        let base = suite(&[("withdraw", 1000, 2, 5), ("cancel", 500, 1, 5)]);
        let now = suite(&[("withdraw", 1040, 3, 5), ("cancel", 500, 1, 5)]);
        let v = verdict(run(&base, &now, "5%"));
        assert!(!v.passed());
        // Instructions are +4%, inside the tolerance; write_entries are +50%.
        assert_eq!(v.regressions.len(), 2);
        assert!(v
            .regressions
            .iter()
            .all(|f| f.handler == "withdraw" && f.resource == Resource::WriteEntries));
        let kinds: Vec<_> = v.regressions.iter().map(|f| f.stat).collect();
        assert_eq!(kinds, [StatKind::Max, StatKind::P50]);
        assert_eq!(v.regressions[0].delta_text(), "+50.0%");
    }

    #[test]
    fn a_larger_tolerance_absorbs_the_same_change() {
        let base = suite(&[("withdraw", 1000, 2, 5)]);
        let now = suite(&[("withdraw", 1040, 3, 5)]);
        assert!(verdict(run(&base, &now, "50%")).passed());
        assert!(!verdict(run(&base, &now, "49.99%")).passed());
    }

    #[test]
    fn improvements_are_reported_but_do_not_fail() {
        let base = suite(&[("withdraw", 1000, 4, 5)]);
        let now = suite(&[("withdraw", 800, 4, 5)]);
        let v = verdict(run(&base, &now, "5%"));
        assert!(v.passed());
        assert_eq!(v.improvements.len(), 2);
        assert_eq!(v.improvements[0].delta_text(), "-20.0%");
    }

    #[test]
    fn a_changed_sample_count_fails_even_when_the_numbers_match() {
        let base = suite(&[("withdraw", 1000, 2, 5)]);
        let now = suite(&[("withdraw", 1000, 2, 4)]);
        let v = verdict(run(&base, &now, "5%"));
        assert!(!v.passed());
        assert!(v.regressions.is_empty());
        assert_eq!(
            v.sample_changes,
            [SampleChange {
                handler: "withdraw".into(),
                baseline: 5,
                current: 4
            }]
        );
    }

    #[test]
    fn a_zero_baseline_that_grows_is_new() {
        let base = suite(&[("withdraw", 1000, 0, 5)]);
        let now = suite(&[("withdraw", 1000, 1, 5)]);
        let v = verdict(run(&base, &now, "100%"));
        assert_eq!(v.regressions.len(), 2);
        assert_eq!(v.regressions[0].delta_text(), "new");
    }

    #[test]
    fn anything_that_changes_the_workload_makes_the_snapshot_stale() {
        let base = suite(&[("withdraw", 1000, 2, 5)]);
        let stale = |now: SuiteSnapshot| match run(&base, &now, "5%") {
            Comparison::Stale(why) => why,
            other => panic!("expected stale, got {other:?}"),
        };
        let mut now = base.clone();
        now.seed = 8;
        assert!(stale(now)[0].contains("seed"));
        let mut now = base.clone();
        now.workload_digest = "fnv1a64:00000000000000bb".into();
        assert!(stale(now)[0].contains("digest"));
        let mut now = base.clone();
        now.auth = "real".into();
        assert!(stale(now)[0].contains("auth"));
        let mut now = base.clone();
        now.steps = "1..=5".into();
        now.cases = 4;
        assert_eq!(stale(now).len(), 2);
        assert!(stale(suite(&[])).iter().any(|w| w.contains("not measured")));
        assert!(
            stale(suite(&[("withdraw", 1000, 2, 5), ("extra", 1, 1, 1)]))
                .iter()
                .any(|w| w.contains("not in the snapshot"))
        );
    }

    #[test]
    fn another_cost_model_makes_the_snapshot_stale() {
        let s = suite(&[("withdraw", 1000, 2, 5)]);
        let newer = Toolchain {
            soroban_env_host: "28.0.3".into(),
            ..toolchain()
        };
        match compare(&toolchain(), &s, &newer, &s, Tolerance::default()) {
            Comparison::Stale(why) => assert!(why[0].contains("soroban-env-host")),
            other => panic!("expected stale, got {other:?}"),
        }
        // The core library version alone does not.
        let core = Toolchain {
            breakwater: "0.2.0".into(),
            ..toolchain()
        };
        assert!(matches!(
            compare(&toolchain(), &s, &core, &s, Tolerance::default()),
            Comparison::Compared(_)
        ));
    }
    fn tiered(spec: &str) -> Tolerances {
        Tolerances::parse(spec).unwrap_or_else(|e| panic!("{spec}: {e}"))
    }

    fn run_tiered(baseline: &SuiteSnapshot, current: &SuiteSnapshot, spec: &str) -> Verdict {
        verdict(compare_tiered(
            &toolchain(),
            baseline,
            &toolchain(),
            current,
            &tiered(spec),
        ))
    }

    #[test]
    fn parses_tiered_tolerances() {
        use ToleranceSource as Src;
        let at = |spec: &str, resource: Resource| {
            let (t, source) = tiered(spec).resolve(resource);
            (t.basis_points(), source)
        };
        let storage = Src::Class(ResourceClass::Storage);
        let compute = Src::Class(ResourceClass::Compute);

        // A single bare value is the old behaviour: every resource, as before.
        for resource in Resource::ALL {
            assert_eq!(at("2.5%", resource), (250, Src::All), "{resource:?}");
            assert_eq!(at(" 0 ", resource), (0, Src::All), "{resource:?}");
        }
        assert_eq!(tiered("5%"), Tolerances::uniform(Tolerance::default()));

        let spec = "storage=0%,compute=5%";
        assert_eq!(at(spec, Resource::WriteBytes), (0, storage));
        assert_eq!(at(spec, Resource::TemporaryEntryRentBumps), (0, storage));
        assert_eq!(at(spec, Resource::Instructions), (500, compute));
        assert_eq!(at(spec, Resource::MemBytes), (500, compute));

        // Spaces around items and around `=` do not matter, order does not either.
        let spaced = " compute = 5% , storage = 0 ";
        assert_eq!(tiered(spaced), tiered(spec));
        assert_eq!(tiered("compute=5%,storage=0%"), tiered(spec));

        // What is not mentioned falls back to the default of 5%.
        assert_eq!(
            at("compute=3%", Resource::WriteEntries),
            (500, Src::Default)
        );
        assert_eq!(at("compute=3%", Resource::Instructions), (300, compute));

        // A bare value is the base that classes and resources override.
        assert_eq!(at("5%,storage=0%", Resource::WriteBytes), (0, storage));
        assert_eq!(at("5%,storage=0%", Resource::MemBytes), (500, Src::All));
        assert_eq!(
            at("instructions=0.25%", Resource::Instructions),
            (25, Src::Resource)
        );
    }

    #[test]
    fn a_resource_beats_its_class_which_beats_everything() {
        let spec = "9%,storage=2%,compute=3%,write_bytes=1%,instructions=4%";
        let t = tiered(spec);
        let bps = |r: Resource| t.for_resource(r).basis_points();
        assert_eq!(bps(Resource::WriteBytes), 100, "resource over class");
        assert_eq!(bps(Resource::WriteEntries), 200, "class over everything");
        assert_eq!(bps(Resource::Instructions), 400, "resource over class");
        assert_eq!(bps(Resource::MemBytes), 300, "class over everything");

        // Without the class entries the bare value shows through ...
        let t = tiered("9%,write_bytes=1%");
        assert_eq!(t.for_resource(Resource::WriteBytes).basis_points(), 100);
        assert_eq!(t.for_resource(Resource::MemBytes).basis_points(), 900);
        // ... and without it the default does.
        let t = tiered("write_bytes=1%");
        assert_eq!(t.for_resource(Resource::MemBytes).basis_points(), 500);
        // The resource beats the class whichever way round they are written.
        assert_eq!(
            tiered("instructions=4%,compute=3%"),
            tiered("compute=3%,instructions=4%")
        );
        // The builder agrees with the parser.
        let built = Tolerances::uniform(Tolerance::parse("9%").unwrap())
            .with_class(ResourceClass::Storage, Tolerance::parse("2%").unwrap())
            .with_class(ResourceClass::Compute, Tolerance::parse("3%").unwrap())
            .with_resource(Resource::WriteBytes, Tolerance::parse("1%").unwrap())
            .with_resource(Resource::Instructions, Tolerance::parse("4%").unwrap());
        assert_eq!(built, tiered(spec));
    }

    #[test]
    fn rejects_bad_tolerance_lists() {
        let keys = "storage, compute, instructions, mem_bytes, disk_read_entries";
        let err = |spec: &str| match Tolerances::parse(spec) {
            Ok(t) => panic!("`{spec}` was accepted as {t}"),
            Err(e) => e,
        };
        for spec in ["", ",", "5%,", ",5%", "storage=0%,,compute=5%", " , "] {
            assert!(matches!(err(spec), TolerancesError::EmptyItem), "{spec:?}");
        }
        for spec in ["=5%", " =5%", "storage=0%,=5%"] {
            assert!(matches!(err(spec), TolerancesError::MissingKey), "{spec:?}");
        }
        for (spec, key) in [
            ("cpu=5%", "cpu"),
            ("Storage=0%", "Storage"),
            ("COMPUTE=5%", "COMPUTE"),
            ("fee=1%", "fee"),
            ("storage=0%,tx_size=1%", "tx_size"),
        ] {
            match err(spec) {
                TolerancesError::UnknownKey(found) => assert_eq!(found, key),
                other => panic!("{spec}: {other:?}"),
            }
        }
        for (spec, key) in [
            ("storage=", "storage"),
            ("storage=lots", "storage"),
            ("compute=-1%", "compute"),
            ("compute=1.234%", "compute"),
            ("instructions=5%%", "instructions"),
            ("compute=42949673%", "compute"),
        ] {
            match err(spec) {
                TolerancesError::Value {
                    key: Some(found), ..
                } => assert_eq!(found, key, "{spec}"),
                other => panic!("{spec}: {other:?}"),
            }
        }
        for spec in ["lots", "storage", "-1%", "5%%", "42949673%"] {
            assert!(
                matches!(err(spec), TolerancesError::Value { key: None, .. }),
                "{spec}"
            );
        }
        for (spec, key) in [
            ("storage=0%,storage=1%", "storage"),
            ("compute=5%,compute=5%", "compute"),
            ("instructions=1%,instructions=2%", "instructions"),
            ("5%,compute=1%,mem_bytes=1%,mem_bytes=2%", "mem_bytes"),
        ] {
            match err(spec) {
                TolerancesError::DuplicateKey(found) => assert_eq!(found, key, "{spec}"),
                other => panic!("{spec}: {other:?}"),
            }
        }
        for spec in ["5%,5%", "5%,storage=0%,1%"] {
            assert!(matches!(err(spec), TolerancesError::DuplicateAll), "{spec}");
        }
        // The largest percentage that fits in basis points is still fine.
        assert!(Tolerances::parse("storage=42949672%").is_ok());

        // The messages name the problem and list every key that would work.
        for spec in ["cpu=5%", "=5%", "storage"] {
            let message = err(spec).to_string();
            assert!(message.contains(keys), "{spec}: {message}");
        }
        assert!(err("cpu=5%").to_string().contains("`cpu`"));
        assert!(err("storage=0%,storage=1%")
            .to_string()
            .contains("`storage`"));
        // A bare value that is not a percentage keeps the original wording.
        assert!(err("lots")
            .to_string()
            .starts_with("invalid tolerance `lots`: expected a non-negative percentage"));
    }

    #[test]
    fn tolerances_are_displayed_for_the_report_header() {
        let shown = |spec: &str| tiered(spec).to_string();
        // Unchanged for a single value, and for no value at all.
        assert_eq!(shown("5%"), "5%");
        assert_eq!(shown("0"), "0%");
        assert_eq!(shown("2.5%"), "2.5%");
        assert_eq!(Tolerances::default().to_string(), "5%");
        assert_eq!(shown("storage=0%,compute=5%"), "storage 0%, compute 5%");
        assert_eq!(shown("compute=3%"), "storage 5% (default), compute 3%");
        assert_eq!(shown("2%,compute=3%"), "storage 2%, compute 3%");
        assert_eq!(
            shown("storage=0%,compute=5%,instructions=0.25%"),
            "storage 0%, compute 5%, instructions 0.25%"
        );
    }

    #[test]
    fn each_class_is_judged_by_its_own_tolerance() {
        // Storage: write_entries 100 -> 101 is +1%. Compute: instructions
        // 1000 -> 1050 is +5%, and 1051 is just over.
        let base = suite(&[("h", 1000, 100, 5)]);
        let now = |instructions: i64, writes: i64| suite(&[("h", instructions, writes, 5)]);
        let spec = "storage=0%,compute=5%";

        assert!(run_tiered(&base, &now(1050, 100), spec).passed());
        let v = run_tiered(&base, &now(1051, 100), spec);
        assert!(!v.passed());
        assert!(v
            .regressions
            .iter()
            .all(|f| f.resource == Resource::Instructions));
        assert_eq!(v.regressions_in(ResourceClass::Compute).count(), 2);
        assert_eq!(v.regressions_in(ResourceClass::Storage).count(), 0);

        // One more write is over a 0% storage tolerance, with compute loose.
        let v = run_tiered(&base, &now(1000, 101), "storage=0%,compute=1000%");
        assert_eq!(v.regressions_in(ResourceClass::Storage).count(), 2);
        assert_eq!(v.regressions_in(ResourceClass::Compute).count(), 0);
        // ... and within a 1% one, exactly at the edge.
        assert!(run_tiered(&base, &now(1000, 101), "storage=1%").passed());
        assert!(!run_tiered(&base, &now(1000, 102), "storage=1%").passed());

        // The same change fails a strict compute tolerance and passes a lax one.
        assert!(!run_tiered(&base, &now(1040, 100), "compute=3.99%").passed());
        assert!(run_tiered(&base, &now(1040, 100), "compute=4%").passed());

        // A resource named on its own overrides its class.
        assert!(run_tiered(&base, &now(1100, 100), "compute=0%,instructions=10%").passed());
        assert!(!run_tiered(&base, &now(1101, 100), "compute=0%,instructions=10%").passed());
    }

    #[test]
    fn a_zero_baseline_allows_only_zero_in_every_tier() {
        let base = suite(&[("h", 0, 0, 5)]);
        let now = suite(&[("h", 1, 1, 5)]);
        let v = run_tiered(&base, &now, "storage=100%,compute=100%");
        assert_eq!(v.regressions_in(ResourceClass::Storage).count(), 2);
        assert_eq!(v.regressions_in(ResourceClass::Compute).count(), 2);
        assert!(run_tiered(&base, &base, "storage=0%,compute=0%").passed());
    }

    #[test]
    fn an_improvement_is_judged_by_the_tolerance_of_its_class() {
        let base = suite(&[("h", 1000, 100, 5)]);
        // instructions -3% and write_entries -1%.
        let now = suite(&[("h", 970, 99, 5)]);

        let v = run_tiered(&base, &now, "storage=0%,compute=5%");
        assert!(v.passed());
        assert_eq!(v.improvements_in(ResourceClass::Storage).count(), 2);
        assert_eq!(v.improvements_in(ResourceClass::Compute).count(), 0);

        let v = run_tiered(&base, &now, "storage=5%,compute=0%");
        assert_eq!(v.improvements_in(ResourceClass::Storage).count(), 0);
        assert_eq!(v.improvements_in(ResourceClass::Compute).count(), 2);

        // The two tolerances can be applied to a larger drop at once.
        let v = run_tiered(&base, &suite(&[("h", 900, 90, 5)]), "storage=5%,compute=5%");
        assert_eq!(v.improvements.len(), 4);
    }

    #[test]
    fn a_uniform_tiered_comparison_is_the_plain_comparison() {
        let base = suite(&[("withdraw", 1000, 4, 5), ("cancel", 500, 1, 5)]);
        let cases = [
            suite(&[("withdraw", 1000, 4, 5), ("cancel", 500, 1, 5)]),
            suite(&[("withdraw", 1040, 5, 5), ("cancel", 500, 1, 5)]),
            suite(&[("withdraw", 800, 4, 5), ("cancel", 520, 1, 4)]),
            suite(&[("withdraw", 1000, 4, 5), ("cancel", 500, 0, 5)]),
            suite(&[("withdraw", 2000, 9, 5), ("cancel", 1, 1, 5)]),
        ];
        for text in ["0%", "1%", "2.5%", "5%", "50%", "10000%"] {
            let tolerance = Tolerance::parse(text).unwrap();
            for now in &cases {
                assert_eq!(
                    compare(&toolchain(), &base, &toolchain(), now, tolerance),
                    compare_tiered(
                        &toolchain(),
                        &base,
                        &toolchain(),
                        now,
                        &Tolerances::uniform(tolerance)
                    ),
                    "{text}"
                );
                assert_eq!(
                    compare(&toolchain(), &base, &toolchain(), now, tolerance),
                    compare_tiered(&toolchain(), &base, &toolchain(), now, &tiered(text)),
                    "{text}"
                );
            }
        }
    }

    #[test]
    fn a_stale_snapshot_is_stale_whatever_the_tolerances() {
        let base = suite(&[("withdraw", 1000, 2, 5)]);
        let mut now = base.clone();
        now.seed = 8;
        assert!(matches!(
            compare_tiered(
                &toolchain(),
                &base,
                &toolchain(),
                &now,
                &tiered("storage=0%")
            ),
            Comparison::Stale(_)
        ));
        // And a changed sample count fails in every tier.
        let v = run_tiered(
            &base,
            &suite(&[("withdraw", 1000, 2, 4)]),
            "storage=100%,compute=100%",
        );
        assert!(!v.passed());
        assert!(v.regressions.is_empty());
    }

    #[test]
    fn findings_know_their_class() {
        let base = suite(&[("h", 1000, 2, 5)]);
        let v = run_tiered(&base, &suite(&[("h", 2000, 3, 5)]), "0%");
        let classes: Vec<_> = v.regressions.iter().map(Finding::class).collect();
        assert_eq!(
            classes
                .iter()
                .filter(|c| **c == ResourceClass::Storage)
                .count(),
            2
        );
        assert_eq!(
            classes
                .iter()
                .filter(|c| **c == ResourceClass::Compute)
                .count(),
            2
        );
    }
}
