//! Freshness check: mails an alert when the archive or the Pi's live data goes
//! stale, and again when it recovers.  Runs on the archive host so it still
//! works when the Pi is down.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;

use chrono::NaiveDate;

const SECONDS_PER_MINUTE: f64 = 60.0;
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// Age in seconds of the newest live sample on the Pi.
const LIVE_AGE_QUERY: &str = "time() - max(timestamp(renogy_soc_percent_value))";

/// What the check looks at.  The set of kinds with problems is persisted so
/// mail goes out only when it changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Archive,
    Live,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Archive => "archive",
            Kind::Live => "live",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "archive" => Some(Kind::Archive),
            "live" => Some(Kind::Live),
            _ => None,
        }
    }
}

/// A detected problem and its human-readable detail.
#[derive(Debug)]
struct Problem {
    kind: Kind,
    detail: String,
}

/// Inputs to one check run.
pub(crate) struct CheckConfig<'a> {
    /// Newest archived day, or `None` if the archive has no files yet.
    pub(crate) newest_archived: Option<NaiveDate>,
    pub(crate) today: NaiveDate,
    pub(crate) max_archive_age_days: i64,
    /// VictoriaMetrics base URL on the Pi; the live check is skipped if unset.
    pub(crate) vm_url: Option<&'a str>,
    pub(crate) max_live_age_minutes: i64,
    pub(crate) alert_email: &'a str,
    pub(crate) sendmail: &'a Path,
    /// Holds the kinds that had problems on the previous run, one per line.
    pub(crate) state_file: &'a Path,
}

/// Runs the check, mailing on any change in the set of problems.  Returns
/// whether everything is healthy.
pub(crate) fn run(cfg: &CheckConfig) -> Result<bool, Box<dyn std::error::Error>> {
    let problems: Vec<Problem> = [
        archive_problem(cfg.newest_archived, cfg.today, cfg.max_archive_age_days),
        cfg.vm_url
            .and_then(|url| live_problem(url, cfg.max_live_age_minutes)),
    ]
    .into_iter()
    .flatten()
    .collect();
    for p in &problems {
        tracing::warn!("{}: {}", p.kind.as_str(), p.detail);
    }

    let previous = read_state(cfg.state_file)?;
    let current: BTreeSet<Kind> = problems.iter().map(|p| p.kind).collect();
    if current != previous {
        let (subject, body) = message(&problems, &previous);
        send_mail(cfg.sendmail, cfg.alert_email, &subject, &body)?;
        tracing::info!("Mailed {}: {subject}", cfg.alert_email);
        write_state(cfg.state_file, &current)?;
    }
    Ok(problems.is_empty())
}

fn archive_problem(newest: Option<NaiveDate>, today: NaiveDate, max_days: i64) -> Option<Problem> {
    let Some(newest) = newest else {
        tracing::info!("archive: no files yet; skipping");
        return None;
    };
    let age = (today - newest).num_days();
    (age > max_days).then(|| Problem {
        kind: Kind::Archive,
        detail: format!("newest archived day is {newest} ({age} days old; limit {max_days})"),
    })
}

fn live_problem(vm_url: &str, max_minutes: i64) -> Option<Problem> {
    let problem = |detail| {
        Some(Problem {
            kind: Kind::Live,
            detail,
        })
    };
    match live_age_seconds(vm_url) {
        Err(e) => problem(format!(
            "cannot query {vm_url}: {}",
            error_chain(e.as_ref())
        )),
        Ok(None) => problem(format!("no recent samples in {vm_url}")),
        Ok(Some(age)) => {
            let minutes = age / SECONDS_PER_MINUTE;
            if minutes > max_minutes as f64 {
                problem(format!(
                    "newest sample is {minutes:.0} minutes old (limit {max_minutes})"
                ))
            } else {
                None
            }
        }
    }
}

/// Formats an error with its sources, e.g. "error sending request: ... Connection refused".
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut text = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        text.push_str(&format!(": {s}"));
        source = s.source();
    }
    text
}

/// Queries the age of the newest sample.  `None` means VictoriaMetrics
/// answered but has no sample within its lookback window.
fn live_age_seconds(vm_url: &str) -> Result<Option<f64>, Box<dyn std::error::Error>> {
    let url = format!("{}/api/v1/query", vm_url.trim_end_matches('/'));
    let text = reqwest::blocking::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()?
        .get(url)
        .query(&[("query", LIVE_AGE_QUERY)])
        .send()?
        .error_for_status()?
        .text()?;
    let body: serde_json::Value = serde_json::from_str(&text)?;
    // Instant vector: data.result[0].value = [timestamp, "value"].
    let Some(value) = body["data"]["result"][0]["value"][1].as_str() else {
        return Ok(None);
    };
    Ok(Some(value.parse()?))
}

fn message(problems: &[Problem], previous: &BTreeSet<Kind>) -> (String, String) {
    let current: BTreeSet<Kind> = problems.iter().map(|p| p.kind).collect();
    let resolved: Vec<&str> = previous.difference(&current).map(|k| k.as_str()).collect();
    let subject = if problems.is_empty() {
        "[renogymon] OK: archive and live data healthy".to_string()
    } else {
        let kinds: Vec<&str> = current.iter().map(|k| k.as_str()).collect();
        format!("[renogymon] ALERT: {} stale", kinds.join(" and "))
    };
    let mut body = String::new();
    for p in problems {
        body.push_str(&format!("PROBLEM {}: {}\n", p.kind.as_str(), p.detail));
    }
    for k in resolved {
        body.push_str(&format!("RESOLVED {k}\n"));
    }
    (subject, body)
}

fn send_mail(
    sendmail: &Path,
    to: &str,
    subject: &str,
    body: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut child = Command::new(sendmail)
        .arg(to)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run {}: {e}", sendmail.display()))?;
    child
        .stdin
        .take()
        .ok_or("sendmail stdin unavailable")?
        .write_all(format!("To: {to}\nSubject: {subject}\n\n{body}").as_bytes())?;
    let status = child.wait()?;
    if !status.success() {
        return Err(format!("{} exited unsuccessfully: {status}", sendmail.display()).into());
    }
    Ok(())
}

fn read_state(path: &Path) -> std::io::Result<BTreeSet<Kind>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text.lines().filter_map(Kind::parse).collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeSet::new()),
        Err(e) => Err(e),
    }
}

fn write_state(path: &Path, kinds: &BTreeSet<Kind>) -> std::io::Result<()> {
    let text: String = kinds.iter().map(|k| format!("{}\n", k.as_str())).collect();
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn archive_age_limit() {
        let today = day("2026-09-27");
        assert!(archive_problem(Some(day("2026-09-24")), today, 3).is_none());
        assert!(archive_problem(Some(day("2026-09-23")), today, 3).is_some());
        assert!(archive_problem(None, today, 3).is_none());
    }

    #[test]
    fn message_lists_problems_and_resolutions() {
        let problems = [Problem {
            kind: Kind::Live,
            detail: "x".to_string(),
        }];
        let previous = BTreeSet::from([Kind::Archive]);
        let (subject, body) = message(&problems, &previous);
        assert_eq!(subject, "[renogymon] ALERT: live stale");
        assert_eq!(body, "PROBLEM live: x\nRESOLVED archive\n");
        let (subject, body) = message(&[], &previous);
        assert!(subject.contains("OK"));
        assert_eq!(body, "RESOLVED archive\n");
    }
}
