//! Comparing a fresh measurement with a stored snapshot.
//!
//! The comparison has three outcomes. If the two cannot be compared at all
//! (other toolchain, other workload, other handlers) the snapshot is
//! [`Comparison::Stale`]. Otherwise every handler's `max` and `p50` per
//! resource is checked against the baseline within a [`Tolerance`], and the
//! sample counts are compared: with the same generated workload, fewer or more
//! successful calls means the contract's behaviour changed, which would let a
//! handler quietly drop out of the measurement.

use std::fmt;

use crate::resource::Resource;
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
}

/// Whether a snapshot can be compared with a measurement at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Comparison {
    /// The snapshot does not describe this measurement; each entry says why.
    Stale(Vec<String>),
    /// Both describe the same workload; here is how the numbers moved.
    Compared(Verdict),
}

/// Compares `current` with `baseline`.
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
}
