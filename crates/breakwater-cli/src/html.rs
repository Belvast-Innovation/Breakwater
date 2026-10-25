//! Single-file static HTML rendering of a run report: inline CSS, inline SVG,
//! no scripts, no external resources.

use breakwater::{Outcome, RunReport, StepOutcome, StepRecord};

use crate::render::args_summary;

const W: f64 = 760.0;
const H: f64 = 200.0;
const PAD: f64 = 48.0;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn outcome_label(o: &StepOutcome) -> String {
    match o {
        StepOutcome::Success => "Success".into(),
        StepOutcome::ContractError(c) => format!("ContractError(#{c})"),
        StepOutcome::Unauthorized => "Unauthorized".into(),
        StepOutcome::HandlerError(m) => format!("HandlerError({m})"),
        StepOutcome::Panic(m) => format!("Panic({m})"),
    }
}

/// Maps ledger sequence numbers onto the SVG x axis.
struct XScale {
    min: f64,
    max: f64,
}

impl XScale {
    fn new(steps: &[StepRecord]) -> Self {
        let min = steps
            .first()
            .map(|s| f64::from(s.ledger_sequence) - f64::from(s.gap.ledgers))
            .unwrap_or(0.0);
        let max = steps
            .last()
            .map(|s| f64::from(s.ledger_sequence))
            .unwrap_or(1.0);
        Self {
            min,
            max: if max > min { max } else { min + 1.0 },
        }
    }
    fn x(&self, ledger: f64) -> f64 {
        PAD + (ledger - self.min) / (self.max - self.min) * (W - 2.0 * PAD)
    }
}

fn timeline(steps: &[StepRecord], failing: Option<usize>) -> String {
    let xs = XScale::new(steps);
    let y = H / 2.0;
    let mut svg = format!(
        "<svg viewBox=\"0 0 {W} {H}\" role=\"img\" aria-label=\"step timeline\"><line class=\"axis\" x1=\"{}\" y1=\"{y}\" x2=\"{}\" y2=\"{y}\"/>",
        PAD,
        W - PAD
    );
    svg.push_str(&format!(
        "<text class=\"tick\" x=\"{}\" y=\"{}\">ledger {}</text><text class=\"tick end\" x=\"{}\" y=\"{}\">ledger {}</text>",
        PAD,
        y + 28.0,
        xs.min as u64,
        W - PAD,
        y + 28.0,
        xs.max as u64
    ));
    for s in steps {
        let x = xs.x(f64::from(s.ledger_sequence));
        let class = match (&s.outcome, failing == Some(s.index)) {
            (_, true) => "dot fail",
            (StepOutcome::Success, _) => "dot ok",
            _ => "dot rej",
        };
        svg.push_str(&format!(
            "<circle class=\"{class}\" cx=\"{x:.1}\" cy=\"{y}\" r=\"7\"><title>#{} {} {}</title></circle><text class=\"lbl\" x=\"{x:.1}\" y=\"{}\">#{}</text>",
            s.index,
            esc(&s.handler),
            esc(&args_summary(&s.args)),
            y - 14.0,
            s.index
        ));
    }
    svg.push_str("</svg>");
    svg
}

/// Cumulative spend chart for `rolling_cap` violations.
fn spend_chart(report: &RunReport, steps: &[StepRecord]) -> Option<String> {
    let failure = report.failure()?;
    if failure.violation.source != "rolling_cap" {
        return None;
    }
    let detail = failure.violation.detail.as_ref()?;
    let limit = detail.get("limit")?.as_f64()?;
    let field = steps.iter().find_map(|s| {
        s.args
            .as_object()
            .and_then(|m| m.keys().find(|k| k.starts_with("amount")).cloned())
    })?;
    let xs = XScale::new(steps);
    let mut total = 0.0;
    let mut points = vec![(xs.x(xs.min), 0.0)];
    for s in steps {
        if matches!(s.outcome, StepOutcome::Success) {
            total += s.args.get(&field).and_then(|v| v.as_f64()).unwrap_or(0.0);
        }
        points.push((xs.x(f64::from(s.ledger_sequence)), total));
    }
    let max = total.max(limit) * 1.15;
    let y = |v: f64| H - PAD / 2.0 - v / max * (H - PAD);
    let poly: Vec<String> = points
        .iter()
        .map(|(x, v)| format!("{x:.1},{:.1}", y(*v)))
        .collect();
    let mut svg =
        format!("<svg viewBox=\"0 0 {W} {H}\" role=\"img\" aria-label=\"cumulative spend\">");
    svg.push_str(&format!(
        "<line class=\"limit\" x1=\"{}\" y1=\"{:.1}\" x2=\"{}\" y2=\"{:.1}\"/><text class=\"tick\" x=\"{}\" y=\"{:.1}\">limit {limit}</text>",
        PAD,
        y(limit),
        W - PAD,
        y(limit),
        PAD,
        y(limit) - 6.0
    ));
    svg.push_str(&format!(
        "<polyline class=\"series\" points=\"{}\"/>",
        poly.join(" ")
    ));
    for (x, v) in &points[1..] {
        svg.push_str(&format!(
            "<circle class=\"dot ok\" cx=\"{x:.1}\" cy=\"{:.1}\" r=\"4\"/>",
            y(*v)
        ));
    }
    svg.push_str(&format!(
        "<text class=\"tick end\" x=\"{}\" y=\"{:.1}\">spent {total}</text></svg>",
        W - PAD,
        y(total) - 8.0
    ));
    Some(svg)
}

/// TTL chart for `no_mid_lifecycle_expiry` violations, from the detail.
fn ttl_chart(report: &RunReport, steps: &[StepRecord]) -> Option<String> {
    let failure = report.failure()?;
    if failure.violation.source != "no_mid_lifecycle_expiry" {
        return None;
    }
    let detail = failure.violation.detail.as_ref()?;
    let created = detail.get("created_at_ledger")?.as_f64()?;
    let now = detail.get("now_ledger")?.as_f64()?;
    let ttl_now = detail.get("ttl_ledgers")?.as_f64()?;
    // TTL decreases by one per ledger: ttl(l) = ttl_now + (now - l).
    let ttl_at = |l: f64| ttl_now + (now - l);
    let xs = XScale::new(steps);
    let top = ttl_at(created).max(1.0);
    let bottom = ttl_now.min(0.0);
    let span = top - bottom;
    let y = |v: f64| PAD / 2.0 + (top - v) / span * (H - PAD);
    let mut svg =
        format!("<svg viewBox=\"0 0 {W} {H}\" role=\"img\" aria-label=\"ttl over ledgers\">");
    svg.push_str(&format!(
        "<line class=\"limit\" x1=\"{}\" y1=\"{:.1}\" x2=\"{}\" y2=\"{:.1}\"/><text class=\"tick\" x=\"{}\" y=\"{:.1}\">TTL 0 (archived below)</text>",
        PAD,
        y(0.0),
        W - PAD,
        y(0.0),
        PAD,
        y(0.0) - 6.0
    ));
    svg.push_str(&format!(
        "<polyline class=\"series\" points=\"{:.1},{:.1} {:.1},{:.1}\"/>",
        xs.x(created),
        y(ttl_at(created)),
        xs.x(now),
        y(ttl_now)
    ));
    for s in steps {
        let l = f64::from(s.ledger_sequence);
        if l >= created {
            svg.push_str(&format!(
                "<circle class=\"dot ok\" cx=\"{:.1}\" cy=\"{:.1}\" r=\"4\"/>",
                xs.x(l),
                y(ttl_at(l))
            ));
        }
    }
    svg.push_str(&format!(
        "<text class=\"tick end\" x=\"{}\" y=\"{:.1}\">TTL {ttl_now} at the violating step</text></svg>",
        W - PAD,
        y(ttl_now) + 16.0
    ));
    Some(svg)
}

/// The full page.
pub fn render(report: &RunReport) -> String {
    let (status, class, steps, failing) = match &report.outcome {
        Outcome::Held => ("all invariants held".to_owned(), "ok", &[][..], None),
        Outcome::Violated(f) => (
            format!("{} ({})", f.violation.message, f.violation.source),
            "fail",
            f.log.steps(),
            Some(f.failing_step),
        ),
    };
    let mut rows = String::new();
    for s in steps {
        let cls = if failing == Some(s.index) {
            " class=\"fail\""
        } else {
            ""
        };
        rows.push_str(&format!(
            "<tr{cls}><td>#{}</td><td>{}</td><td>{}</td><td>+{}</td><td>{}</td><td><code>{}</code></td><td>{}</td></tr>",
            s.index,
            esc(&s.handler),
            esc(s.actor.name()),
            s.gap.ledgers,
            s.ledger_sequence,
            esc(&args_summary(&s.args)),
            esc(&outcome_label(&s.outcome))
        ));
    }
    let charts = [spend_chart(report, steps), ttl_chart(report, steps)]
        .into_iter()
        .flatten()
        .map(|svg| format!("<section><h2>Violation chart</h2>{svg}</section>"))
        .collect::<String>();
    let timeline = if steps.is_empty() {
        String::new()
    } else {
        format!(
            "<section><h2>Step timeline</h2>{}</section>",
            timeline(steps, failing)
        )
    };
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Breakwater report: {suite}</title>
<style>
:root{{--bg:#fff;--fg:#1b1f24;--muted:#6a737d;--ok:#1a7f37;--fail:#cf222e;--rej:#9a6700;--line:#d0d7de;--accent:#0969da}}
@media (prefers-color-scheme:dark){{:root{{--bg:#0d1117;--fg:#e6edf3;--muted:#8b949e;--ok:#3fb950;--fail:#f85149;--rej:#d29922;--line:#30363d;--accent:#58a6ff}}}}
body{{margin:0;padding:24px 16px;background:var(--bg);color:var(--fg);font:15px/1.5 ui-sans-serif,system-ui,sans-serif;max-width:800px;margin-inline:auto}}
h1{{font-size:22px;margin:0 0 4px}}h2{{font-size:16px;margin:28px 0 8px}}
.meta{{color:var(--muted)}}.status{{padding:10px 14px;border-radius:8px;margin:16px 0;font-weight:600}}
.status.ok{{background:color-mix(in srgb,var(--ok) 15%,transparent);color:var(--ok)}}.status.fail{{background:color-mix(in srgb,var(--fail) 15%,transparent);color:var(--fail)}}
table{{border-collapse:collapse;width:100%;font-size:14px}}th,td{{text-align:left;padding:6px 8px;border-bottom:1px solid var(--line)}}tr.fail td{{color:var(--fail);font-weight:600}}
code{{font:13px ui-monospace,monospace}}svg{{width:100%;height:auto;display:block}}
.axis,.limit{{stroke:var(--line);stroke-width:2}}.limit{{stroke:var(--fail);stroke-dasharray:6 4}}
.series{{fill:none;stroke:var(--accent);stroke-width:2.5}}.dot.ok{{fill:var(--ok)}}.dot.rej{{fill:var(--rej)}}.dot.fail{{fill:var(--fail);stroke:var(--fg);stroke-width:2}}
.tick,.lbl{{fill:var(--muted);font-size:12px}}.tick.end{{text-anchor:end}}.lbl{{text-anchor:middle}}
</style></head><body>
<h1>{suite}</h1>
<p class="meta">seed {seed} · {cases} cases · {steps_run} steps</p>
<div class="status {class}">{status}</div>
{timeline}
<section><h2>Steps</h2><table><thead><tr><th>#</th><th>handler</th><th>actor</th><th>gap</th><th>ledger</th><th>args</th><th>outcome</th></tr></thead><tbody>{rows}</tbody></table></section>
{charts}
<p class="meta">Generated by <code>breakwater report --html</code>. No scripts, no external resources.</p>
</body></html>
"#,
        suite = esc(&report.suite),
        seed = report.seed,
        cases = report.cases_run,
        steps_run = report.steps_run,
        status = esc(&status),
    )
}
