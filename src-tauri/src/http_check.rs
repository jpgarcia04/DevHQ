//! Read-only HTTP(S) checks. The request runs in Rust, never in the WebView, when the person
//! asks for it or, if they turned supervision on, every interval while DevHQ is open (see
//! `monitor`). A failure of the checked system (DNS, refused connection, TLS, timeout,
//! unexpected status) is evidence: an unhealthy observation. A failure of DevHQ itself (invalid
//! or missing configuration, SQLite) is an error and is never recorded.

use crate::inventory::{open, ApiError};
use crate::records::{
    self, HttpEvidence, Lifeline, NewObservation, Observation, Redirect, Subject,
};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::header::LOCATION;
use reqwest::{redirect, Client, Url};
use rusqlite::{params, OptionalExtension, Row, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error as StdError;
use std::io::ErrorKind;
use std::path::Path;
use std::time::{Duration, Instant};

/// Redirects followed before the check gives up. Every hop is recorded.
const MAX_REDIRECTS: usize = 5;
const MAX_TIMEOUT_SECONDS: i64 = 10;
const MAX_URL_LENGTH: usize = 2048;
/// Supervision runs a check at most every 5 minutes and at least every hour.
const MONITOR_INTERVALS: std::ops::RangeInclusive<i64> = 5..=60;
const CHECK_SELECT: &str = "SELECT application_id, resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at, monitor, monitor_interval_minutes FROM http_checks";
const SUBJECT_FILTER: &str = "WHERE application_id IS ?1 AND resource_id IS ?2";

/// The one HTTP check an application or resource can have.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HttpCheck {
    pub subject: Subject,
    pub url: String,
    pub expected_status: i64,
    pub timeout_seconds: i64,
    pub valid_for_minutes: i64,
    pub updated_at: String,
    /// `off` (runs only when asked), `active` (also every interval while DevHQ is open) or
    /// `paused` (keeps its interval, runs only when asked until resumed).
    pub monitor: String,
    pub monitor_interval_minutes: Option<i64>,
}

#[derive(Debug, Deserialize)]
// Serialized only by tests: ipc_contract finds the fixture keys Rust never reads.
#[cfg_attr(test, derive(Serialize))]
#[serde(rename_all = "camelCase")]
pub struct HttpCheckInput {
    pub subject: Subject,
    pub url: String,
    pub expected_status: i64,
    pub timeout_seconds: i64,
    pub valid_for_minutes: i64,
    /// None turns supervision off. Turning it on starts it active; editing a supervised check
    /// keeps it active or paused as it was.
    pub monitor_interval_minutes: Option<i64>,
}

fn check_from_row(row: &Row) -> rusqlite::Result<HttpCheck> {
    Ok(HttpCheck {
        subject: Subject::from_columns(row.get(0)?, row.get(1)?)?,
        url: row.get(2)?,
        expected_status: row.get(3)?,
        timeout_seconds: row.get(4)?,
        valid_for_minutes: row.get(5)?,
        updated_at: records::time_at(row, 6)?,
        monitor: row.get(7)?,
        monitor_interval_minutes: row.get(8)?,
    })
}

pub(crate) fn load(tx: &Transaction) -> Result<Vec<HttpCheck>, ApiError> {
    let mut statement = tx
        .prepare(&format!("{CHECK_SELECT} ORDER BY rowid"))
        .map_err(ApiError::database)?;
    let checks = statement
        .query_map([], check_from_row)
        .map_err(ApiError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(ApiError::database);
    checks
}

fn find(tx: &Transaction, subject: &Subject) -> Result<Option<HttpCheck>, ApiError> {
    let (application_id, resource_id) = subject.columns();
    tx.query_row(
        &format!("{CHECK_SELECT} {SUBJECT_FILTER}"),
        params![application_id, resource_id],
        check_from_row,
    )
    .optional()
    .map_err(ApiError::database)
}

fn not_configured(subject: &Subject) -> ApiError {
    ApiError::invalid(format!(
        "No HTTP check is configured for this {}. Configure one first.",
        subject.label()
    ))
}

/// A plain http(s) URL. Credentials are refused because DevHQ sends none; the fragment is
/// dropped because it is never sent.
fn endpoint_url(value: &str) -> Result<Url, ApiError> {
    let text = value.trim();
    if text.is_empty() {
        return Err(ApiError::invalid("The endpoint URL is required."));
    }
    if text.len() > MAX_URL_LENGTH {
        return Err(ApiError::invalid(format!(
            "The endpoint URL is longer than {MAX_URL_LENGTH} characters."
        )));
    }
    let mut url = Url::parse(text)
        .map_err(|error| ApiError::invalid(format!("The endpoint URL is not valid: {error}.")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ApiError::invalid(
            "The endpoint URL must start with http:// or https://.",
        ));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(ApiError::invalid("The endpoint URL needs a host."));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ApiError::invalid(
            "Credentials in the URL are not supported. DevHQ sends no credentials.",
        ));
    }
    url.set_fragment(None);
    Ok(url)
}

fn validate(input: &HttpCheckInput) -> Result<Url, ApiError> {
    let url = endpoint_url(&input.url)?;
    if !(200..=599).contains(&input.expected_status) || (300..=399).contains(&input.expected_status)
    {
        return Err(ApiError::invalid(
            "The expected status must be a final HTTP status from 200 to 599. Redirects are followed, so a 3xx status cannot be expected.",
        ));
    }
    if !(1..=MAX_TIMEOUT_SECONDS).contains(&input.timeout_seconds) {
        return Err(ApiError::invalid(format!(
            "The timeout must be between 1 and {MAX_TIMEOUT_SECONDS} seconds."
        )));
    }
    if !(1..=records::MAX_VALIDITY_MINUTES).contains(&input.valid_for_minutes) {
        return Err(ApiError::invalid(
            "Validity must be between 1 minute and 30 days.",
        ));
    }
    if let Some(interval) = input.monitor_interval_minutes {
        if !MONITOR_INTERVALS.contains(&interval) {
            return Err(ApiError::invalid(
                "The supervision interval must be between 5 and 60 minutes.",
            ));
        }
        if input.valid_for_minutes <= interval {
            return Err(ApiError::invalid(format!(
                "The result must stay valid for longer than the supervision interval ({interval} min), or the evidence would expire before the next run. Choose a longer validity or a shorter interval."
            )));
        }
    }
    Ok(url)
}

/// Configures the subject's only HTTP check, replacing any previous one. Observations already
/// produced keep the URL and expected status they were checked with. Everything is validated
/// before the database is opened, so a rejected change writes nothing.
pub fn save_http_check(path: &Path, input: HttpCheckInput) -> Result<HttpCheck, ApiError> {
    let url = validate(&input)?;
    let mut db = open(path)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(ApiError::database)?;
    records::ensure_subject(&tx, &input.subject)?;
    let previous = find(&tx, &input.subject)?;
    let monitor = match (input.monitor_interval_minutes, previous) {
        (None, _) => "off",
        (Some(_), Some(previous)) if previous.monitor == "paused" => "paused",
        (Some(_), _) => "active",
    };
    let (application_id, resource_id) = input.subject.columns();
    tx.execute(
        &format!("DELETE FROM http_checks {SUBJECT_FILTER}"),
        params![application_id, resource_id],
    )
    .map_err(ApiError::database)?;
    tx.execute(
        "INSERT INTO http_checks (application_id, resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at, monitor, monitor_interval_minutes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            application_id,
            resource_id,
            url.as_str(),
            input.expected_status,
            input.timeout_seconds,
            input.valid_for_minutes,
            records::now_ms(),
            monitor,
            input.monitor_interval_minutes
        ],
    )
    .map_err(ApiError::database)?;
    let check = find(&tx, &input.subject)?
        .ok_or_else(|| ApiError::database("HTTP check was not saved."))?;
    tx.commit().map_err(ApiError::database)?;
    Ok(check)
}

/// Pauses or resumes the supervision of the subject's HTTP check. It keeps its interval.
pub fn set_http_check_paused(
    path: &Path,
    subject: &Subject,
    paused: bool,
) -> Result<HttpCheck, ApiError> {
    let mut db = open(path)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(ApiError::database)?;
    let check = find(&tx, subject)?.ok_or_else(|| not_configured(subject))?;
    if check.monitor == "off" {
        return Err(ApiError::invalid(
            "Supervision is off for this check. Choose an interval in Edit check first.",
        ));
    }
    let (application_id, resource_id) = subject.columns();
    tx.execute(
        &format!("UPDATE http_checks SET monitor = ?3 {SUBJECT_FILTER}"),
        params![
            application_id,
            resource_id,
            if paused { "paused" } else { "active" }
        ],
    )
    .map_err(ApiError::database)?;
    let check = find(&tx, subject)?.ok_or_else(|| not_configured(subject))?;
    tx.commit().map_err(ApiError::database)?;
    Ok(check)
}

/// What supervision needs to plan: every check, and when each subject's check last recorded
/// an observation, so that a relaunch keeps the rhythm instead of starting over.
pub(crate) fn supervision(
    path: &Path,
) -> Result<(Vec<HttpCheck>, HashMap<Subject, i64>), ApiError> {
    let mut db = open(path)?;
    let tx = db.transaction().map_err(ApiError::database)?;
    let checks = load(&tx)?;
    let mut last_runs = HashMap::new();
    {
        let prepare = |sql| tx.prepare(sql).map_err(ApiError::database);
        // The newest automated observation, found from the subject's index, so the cost does
        // not grow with history.
        let mut latest = [
            prepare(records::LATEST_AUTOMATED[0])?,
            prepare(records::LATEST_AUTOMATED[1])?,
        ];
        for check in &checks {
            let (kind, id) = check.subject.per_type();
            let at: Option<i64> = latest[kind]
                .query_row(params![id, 1], |row| row.get(1))
                .optional()
                .map_err(ApiError::database)?;
            if let Some(at) = at {
                last_runs.insert(check.subject.clone(), at);
            }
        }
    }
    tx.commit().map_err(ApiError::database)?;
    Ok((checks, last_runs))
}

/// Removes the subject's HTTP check. The observations it produced are evidence and stay.
pub fn remove_http_check(path: &Path, subject: Subject) -> Result<(), ApiError> {
    let db = open(path)?;
    let (application_id, resource_id) = subject.columns();
    let removed = db
        .execute(
            &format!("DELETE FROM http_checks {SUBJECT_FILTER}"),
            params![application_id, resource_id],
        )
        .map_err(ApiError::database)?;
    if removed == 0 {
        return Err(not_configured(&subject));
    }
    Ok(())
}

/// Runs the subject's HTTP check once and records the result, with its timeline event, as an
/// automated observation. One deadline covers the whole request, so a check cannot hang.
/// Scheduled and manual runs are this same run and produce the same evidence; `monitor`
/// makes sure a check never has two at once.
/// Runs the subject's check once and records its observation, while `lifeline` (supervision,
/// which every run goes through) is alive.
pub async fn run_http_check(
    path: &Path,
    subject: Subject,
    lifeline: &Lifeline,
) -> Result<Observation, ApiError> {
    // The database is closed again before any network traffic starts.
    let check = {
        let mut db = open(path)?;
        let tx = db.transaction().map_err(ApiError::database)?;
        records::ensure_subject(&tx, &subject)?;
        find(&tx, &subject)?.ok_or_else(|| not_configured(&subject))?
    };
    let url = endpoint_url(&check.url)?;
    let timeout =
        Duration::from_secs(u64::try_from(check.timeout_seconds).map_err(ApiError::check)?);
    let client = client().map_err(ApiError::check)?;
    let (evidence, summary) = probe(&client, url, check.expected_status, timeout).await;
    let observed_at = records::now_ms();
    records::store_observation(
        path,
        NewObservation {
            subject: &subject,
            result: evidence.result(),
            summary: &summary,
            observed_at,
            valid_for_minutes: check.valid_for_minutes,
            recorded_at: observed_at,
            http: Some(&evidence),
            lifeline: Some(lifeline),
        },
    )
}

fn client() -> Result<Client, reqwest::Error> {
    Client::builder()
        // Followed by hand so that every hop is limited and recorded.
        .redirect(redirect::Policy::none())
        // Straight to the endpoint: no proxy from the environment or the system settings.
        .no_proxy()
        .dns_resolver(Resolver)
        .user_agent(concat!(
            "DevHQ/",
            env!("CARGO_PKG_VERSION"),
            " (HTTP check)"
        ))
        .build()
}

/// Why the checked system did not give a usable answer.
struct Failure {
    label: &'static str,
    detail: String,
}

/// Evidence of one check and a one-line summary for lists and the timeline.
async fn probe(
    client: &Client,
    url: Url,
    expected_status: i64,
    timeout: Duration,
) -> (HttpEvidence, String) {
    let requested = url.to_string();
    let mut redirects = Vec::new();
    let started = Instant::now();
    let outcome = match tokio::time::timeout(timeout, follow(client, url, &mut redirects)).await {
        Ok(outcome) => outcome,
        Err(_) => Err((
            None,
            Failure {
                label: "Timed out",
                detail: format!("no response within {} s", timeout.as_secs()),
            },
        )),
    };
    let latency_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
    let (status, error, summary) = match outcome {
        Ok(status) if status == expected_status => (
            Some(status),
            None,
            format!("HTTP {status} as expected, in {latency_ms} ms"),
        ),
        Ok(status) => (
            Some(status),
            None,
            format!("HTTP {status}, expected {expected_status}, in {latency_ms} ms"),
        ),
        Err((status, Failure { label, detail })) => (
            status,
            Some(format!("{label}: {detail}")),
            format!("{label} after {latency_ms} ms"),
        ),
    };
    let evidence = HttpEvidence {
        url: requested,
        expected_status,
        status,
        latency_ms,
        error,
        redirects,
    };
    (evidence, summary)
}

/// Requests `url` with GET and returns the final status. The body is never read: only the
/// status and, for redirects, the Location header are used. A failure carries the last status
/// received, if any.
async fn follow(
    client: &Client,
    mut url: Url,
    redirects: &mut Vec<Redirect>,
) -> Result<i64, (Option<i64>, Failure)> {
    loop {
        let response = client
            .get(url.clone())
            .send()
            .await
            .map_err(|error| (None, describe(&error)))?;
        let status = response.status();
        let location = response.headers().get(LOCATION).cloned();
        drop(response);
        let code = i64::from(status.as_u16());
        let Some(location) = location.filter(|_| status.is_redirection()) else {
            return Ok(code);
        };
        let next = location
            .to_str()
            .ok()
            .and_then(|value| url.join(value).ok())
            .filter(|next| {
                matches!(next.scheme(), "http" | "https")
                    && next.username().is_empty()
                    && next.password().is_none()
            });
        let Some(next) = next else {
            return Err((
                Some(code),
                Failure {
                    label: "Invalid redirect",
                    detail: format!(
                        "HTTP {code} pointed to a location that is not a plain http(s) URL"
                    ),
                },
            ));
        };
        if redirects.len() == MAX_REDIRECTS {
            return Err((
                Some(code),
                Failure {
                    label: "Too many redirects",
                    detail: format!("stopped after {MAX_REDIRECTS}"),
                },
            ));
        }
        redirects.push(Redirect {
            status: code,
            location: next.to_string(),
        });
        url = next;
    }
}

/// Every error in the chain. `io::Error` hides the error it wraps from `source()`.
fn causes<'a>(error: &'a (dyn StdError + 'static)) -> Vec<&'a (dyn StdError + 'static)> {
    let mut causes = vec![error];
    while let Some(&current) = causes.last() {
        let next = match current
            .downcast_ref::<std::io::Error>()
            .and_then(|io| io.get_ref())
        {
            Some(inner) => Some(inner as &(dyn StdError + 'static)),
            None => current.source(),
        };
        match next {
            Some(next) => causes.push(next),
            None => break,
        }
    }
    causes
}

/// Names the failure by type; the operating system's own wording follows as detail.
fn describe(error: &reqwest::Error) -> Failure {
    let causes = causes(error);
    if let Some(dns) = causes
        .iter()
        .find_map(|cause| cause.downcast_ref::<DnsFailure>())
    {
        return Failure {
            label: "DNS lookup failed",
            detail: dns.to_string(),
        };
    }
    let io_kind = |kind: ErrorKind| {
        causes.iter().any(|cause| {
            cause
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == kind)
        })
    };
    let label = if io_kind(ErrorKind::ConnectionRefused) {
        "Connection refused"
    } else if causes.iter().any(|cause| cause.is::<native_tls::Error>()) {
        "TLS handshake failed"
    } else if io_kind(ErrorKind::TimedOut) || error.is_timeout() {
        "Timed out"
    } else if error.is_connect() {
        "Connection failed"
    } else {
        "Request failed"
    };
    Failure {
        label,
        detail: causes
            .last()
            .map_or_else(|| error.to_string(), ToString::to_string),
    }
}

/// Resolves names like the default resolver, but marks its failures so that a DNS problem can
/// be told apart from other connection errors.
struct Resolver;

#[derive(Debug)]
struct DnsFailure {
    host: String,
    source: std::io::Error,
}

impl std::fmt::Display for DnsFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.host, self.source)
    }
}

impl StdError for DnsFailure {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.source)
    }
}

impl Resolve for Resolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            // reqwest replaces the port with the URL's own.
            let resolved = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map(Iterator::collect::<Vec<_>>);
            match resolved {
                Ok(addresses) => Ok(Box::new(addresses.into_iter()) as Addrs),
                // Converted directly: boxing first would hide the type from downcasts.
                Err(source) => Err(DnsFailure { host, source }.into()),
            }
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::inventory::{
        list, save_application, save_resource, ApplicationInput, ResourceInput,
    };
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use uuid::Uuid;

    const OK: &str = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
    const ERROR: &str =
        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    const NOT_FOUND: &str =
        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    const MOVED: &str = "HTTP/1.1 301 Moved Permanently\r\nLocation: /health\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    const LOOP: &str =
        "HTTP/1.1 302 Found\r\nLocation: /loop\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

    fn path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("devhq-http-test-{}.sqlite3", Uuid::new_v4()))
    }

    /// A local test server that answers by request path; `None` accepts and never answers.
    pub(crate) fn serve() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                std::thread::spawn(move || {
                    let mut request = [0; 4096];
                    let read = stream.read(&mut request).unwrap_or(0);
                    let text = String::from_utf8_lossy(&request[..read]).into_owned();
                    let response = match text.split_whitespace().nth(1).unwrap_or("") {
                        "/health" => Some(OK),
                        "/error" => Some(ERROR),
                        "/old" => Some(MOVED),
                        "/loop" => Some(LOOP),
                        "/hang" => None,
                        _ => Some(NOT_FOUND),
                    };
                    match response {
                        Some(response) => {
                            let _ = stream.write_all(response.as_bytes());
                        }
                        None => std::thread::sleep(Duration::from_secs(30)),
                    }
                });
            }
        });
        origin
    }

    /// A port with nothing listening on it.
    fn closed_port() -> u16 {
        TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    pub(crate) fn resource(path: &Path) -> Subject {
        let saved = save_resource(
            path,
            ResourceInput {
                id: None,
                name: "Host".into(),
                kind: "server".into(),
                description: String::new(),
                operating_system: String::new(),
            },
        )
        .unwrap();
        Subject::Resource { id: saved.id }
    }

    fn application(path: &Path) -> Subject {
        let saved = save_application(
            path,
            ApplicationInput {
                id: None,
                name: "Shop".into(),
                description: String::new(),
                lifecycle: "production".into(),
                version: None,
                components: vec![],
            },
        )
        .unwrap();
        Subject::Application { id: saved.id }
    }

    fn input(subject: &Subject, url: &str) -> HttpCheckInput {
        HttpCheckInput {
            subject: subject.clone(),
            url: url.into(),
            expected_status: 200,
            timeout_seconds: 5,
            valid_for_minutes: 60,
            monitor_interval_minutes: None,
        }
    }

    fn run(path: &Path, subject: &Subject) -> Result<Observation, ApiError> {
        tauri::async_runtime::block_on(run_http_check(path, subject.clone(), &Lifeline::default()))
    }

    fn millis(time: &str) -> i128 {
        time::OffsetDateTime::parse(time, &time::format_description::well_known::Rfc3339)
            .unwrap()
            .unix_timestamp_nanos()
            / 1_000_000
    }

    #[test]
    fn records_the_expected_status_as_evidence_that_survives_reopen() {
        let path = path();
        let origin = serve();
        let host = resource(&path);
        let shop = application(&path);
        let check =
            save_http_check(&path, input(&host, &format!("  {origin}/health#top  "))).unwrap();
        assert_eq!(check.url, format!("{origin}/health"));
        save_http_check(
            &path,
            HttpCheckInput {
                valid_for_minutes: 15,
                ..input(&shop, &format!("{origin}/old"))
            },
        )
        .unwrap();

        let healthy = run(&path, &host).unwrap();
        assert_eq!(
            (healthy.source.as_str(), healthy.result.as_str()),
            ("http", "healthy")
        );
        let evidence = healthy.http.clone().unwrap();
        assert_eq!(evidence.url, format!("{origin}/health"));
        assert_eq!(
            (evidence.status, evidence.expected_status),
            (Some(200), 200)
        );
        assert_eq!(evidence.error, None);
        assert!(evidence.redirects.is_empty());
        assert!(healthy.summary.starts_with("HTTP 200 as expected, in "));
        assert_eq!(
            millis(&healthy.valid_until) - millis(&healthy.observed_at),
            60 * 60_000
        );

        let redirected = run(&path, &shop).unwrap();
        let evidence = redirected.http.clone().unwrap();
        assert_eq!(redirected.result, "healthy");
        assert_eq!(evidence.url, format!("{origin}/old"));
        assert_eq!(
            evidence.redirects,
            vec![Redirect {
                status: 301,
                location: format!("{origin}/health")
            }]
        );
        assert_eq!(
            millis(&redirected.valid_until) - millis(&redirected.observed_at),
            15 * 60_000
        );

        // Removing the configuration keeps the evidence it produced.
        remove_http_check(&path, host.clone()).unwrap();
        // Every call opened and closed its own connection; this read is a fresh reopen.
        let reopened = list(&path).unwrap();
        assert_eq!(reopened.observations.len(), 2);
        assert!(reopened.observations.contains(&healthy));
        assert!(reopened.observations.contains(&redirected));
        assert_eq!(reopened.http_checks.len(), 1);
        assert_eq!(reopened.http_checks[0].subject, shop);
        for observation in [&healthy, &redirected] {
            let event = reopened
                .events
                .iter()
                .find(|event| event.observation_id.as_deref() == Some(observation.id.as_str()))
                .unwrap();
            assert_eq!(event.source, "http");
            assert_eq!(event.kind, "observation");
            assert_eq!(event.title, "HTTP check: Healthy");
            assert_eq!(event.outcome, "success");
            assert_eq!(event.detail, observation.summary);
            assert_eq!(event.occurred_at, observation.observed_at);
            assert_eq!(event.subject, observation.subject);
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn records_failures_of_the_checked_system_as_unhealthy_evidence() {
        let path = path();
        let origin = serve();
        let https_origin = origin.replace("http://", "https://");
        let host = resource(&path);
        let refused = format!("http://127.0.0.1:{}/", closed_port());
        let cases = [
            (
                format!("{origin}/error"),
                5,
                Some(500),
                None,
                "HTTP 500, expected 200",
            ),
            (
                format!("{origin}/hang"),
                1,
                None,
                Some("Timed out: no response within 1 s"),
                "Timed out after ",
            ),
            (
                refused,
                5,
                None,
                Some("Connection refused: "),
                "Connection refused after ",
            ),
            (
                "http://devhq-check.invalid/".into(),
                5,
                None,
                Some("DNS lookup failed: devhq-check.invalid: "),
                "DNS lookup failed after ",
            ),
            // A TLS client talking to a plain HTTP server cannot complete the handshake.
            (
                format!("{https_origin}/health"),
                5,
                None,
                Some("TLS handshake failed: "),
                "TLS handshake failed after ",
            ),
            (
                format!("{origin}/loop"),
                5,
                Some(302),
                Some("Too many redirects: stopped after 5"),
                "Too many redirects after ",
            ),
        ];
        for (url, timeout_seconds, status, error, summary) in &cases {
            save_http_check(
                &path,
                HttpCheckInput {
                    timeout_seconds: *timeout_seconds,
                    ..input(&host, url)
                },
            )
            .unwrap();
            let started = Instant::now();
            let observation = run(&path, &host).unwrap();
            let evidence = observation.http.clone().unwrap();
            assert_eq!(observation.result, "unhealthy", "{url}: {evidence:?}");
            assert_eq!(observation.source, "http");
            assert_eq!(evidence.status, *status, "{url}: {evidence:?}");
            match error {
                Some(error) => assert!(
                    evidence
                        .error
                        .as_deref()
                        .is_some_and(|text| text.starts_with(error)),
                    "{url}: {evidence:?}"
                ),
                None => assert_eq!(evidence.error, None),
            }
            assert!(
                observation.summary.starts_with(summary),
                "{}",
                observation.summary
            );
            // One deadline covers the whole check.
            assert!(started.elapsed() < Duration::from_secs(*timeout_seconds as u64 + 2));
        }
        let snapshot = list(&path).unwrap();
        let timeout = snapshot
            .observations
            .iter()
            .find_map(|item| {
                item.http
                    .as_ref()
                    .filter(|http| http.url.ends_with("/hang"))
            })
            .unwrap();
        assert!((1_000..3_000).contains(&timeout.latency_ms), "{timeout:?}");
        let looping = snapshot
            .observations
            .iter()
            .find_map(|item| {
                item.http
                    .as_ref()
                    .filter(|http| http.url.ends_with("/loop"))
            })
            .unwrap();
        assert_eq!(looping.redirects.len(), 5);
        assert!(looping.redirects.iter().all(|hop| hop.status == 302));
        assert_eq!(snapshot.observations.len(), cases.len());
        assert_eq!(snapshot.events.len(), cases.len());
        assert!(snapshot.events.iter().all(|event| event.source == "http"
            && event.outcome == "failure"
            && event.title == "HTTP check: Unhealthy"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn failures_of_devhq_are_errors_and_never_observations() {
        let path = path();
        let host = resource(&path);
        let unconfigured = run(&path, &host).unwrap_err();
        assert_eq!(unconfigured.code, "invalid_input");
        assert!(unconfigured.message.contains("No HTTP check is configured"));
        let missing = run(&path, &Subject::Application { id: "ghost".into() }).unwrap_err();
        assert_eq!(missing.code, "invalid_input");
        // A stored configuration that no longer validates is refused before any request.
        let db = open(&path).unwrap();
        let Subject::Resource { id } = &host else {
            unreachable!()
        };
        db.execute(
            "INSERT INTO http_checks (resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at)
             VALUES (?1, 'http://', 200, 5, 60, 0)",
            [id],
        )
        .unwrap();
        drop(db);
        let invalid = run(&path, &host).unwrap_err();
        assert_eq!(invalid.code, "invalid_input");
        assert!(invalid.message.contains("not valid"), "{}", invalid.message);
        let snapshot = list(&path).unwrap();
        assert!(snapshot.observations.is_empty());
        assert!(snapshot.events.is_empty());
        // SQLite itself is unavailable: a directory cannot be opened as the database.
        let unavailable = run(&std::env::temp_dir(), &host).unwrap_err();
        assert_eq!(unavailable.code, "database_error");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_invalid_configuration_without_partial_writes() {
        let path = path();
        let host = resource(&path);
        let shop = application(&path);
        let original = save_http_check(&path, input(&host, "https://example.com/health")).unwrap();
        let long = format!("https://example.com/{}", "a".repeat(MAX_URL_LENGTH));
        let invalid = [
            input(&host, ""),
            input(&host, "ftp://example.com/"),
            input(&host, "example.com/health"),
            input(&host, "http://"),
            input(&host, "https://user:secret@example.com/"),
            input(&host, &long),
            HttpCheckInput {
                expected_status: 199,
                ..input(&host, "https://example.com/")
            },
            HttpCheckInput {
                expected_status: 302,
                ..input(&host, "https://example.com/")
            },
            HttpCheckInput {
                expected_status: 600,
                ..input(&host, "https://example.com/")
            },
            HttpCheckInput {
                timeout_seconds: 0,
                ..input(&host, "https://example.com/")
            },
            HttpCheckInput {
                timeout_seconds: 11,
                ..input(&host, "https://example.com/")
            },
            HttpCheckInput {
                valid_for_minutes: 0,
                ..input(&host, "https://example.com/")
            },
            HttpCheckInput {
                valid_for_minutes: records::MAX_VALIDITY_MINUTES + 1,
                ..input(&host, "https://example.com/")
            },
            input(
                &Subject::Resource { id: "ghost".into() },
                "https://example.com/",
            ),
            // The subject type matters: an application ID is not a resource.
            input(
                &Subject::Resource {
                    id: match &shop {
                        Subject::Application { id } => id.clone(),
                        Subject::Resource { .. } => unreachable!(),
                    },
                },
                "https://example.com/",
            ),
        ];
        for attempt in invalid {
            let url = attempt.url.clone();
            let error = save_http_check(&path, attempt).unwrap_err();
            assert_eq!(error.code, "invalid_input", "{url}: {}", error.message);
        }
        let error = remove_http_check(&path, shop).unwrap_err();
        assert_eq!(error.code, "invalid_input");
        let snapshot = list(&path).unwrap();
        assert_eq!(snapshot.http_checks, vec![original]);
        assert!(snapshot.observations.is_empty());
        std::fs::remove_file(path).unwrap();
    }

    fn supervised(subject: &Subject, interval: i64, valid: i64) -> HttpCheckInput {
        HttpCheckInput {
            valid_for_minutes: valid,
            monitor_interval_minutes: Some(interval),
            ..input(subject, "https://example.com/health")
        }
    }

    #[test]
    fn rejects_supervision_whose_evidence_would_lapse_without_partial_writes() {
        let path = path();
        let host = resource(&path);
        let original = save_http_check(&path, supervised(&host, 5, 15)).unwrap();
        assert_eq!(
            (original.monitor.as_str(), original.monitor_interval_minutes),
            ("active", Some(5))
        );
        for (attempt, message) in [
            (
                supervised(&host, 15, 15),
                "longer than the supervision interval (15 min)",
            ),
            (
                supervised(&host, 60, 60),
                "longer than the supervision interval (60 min)",
            ),
            (
                supervised(&host, 30, 15),
                "longer than the supervision interval (30 min)",
            ),
            (supervised(&host, 4, 60), "between 5 and 60 minutes"),
            (supervised(&host, 61, 240), "between 5 and 60 minutes"),
            (supervised(&host, 0, 60), "between 5 and 60 minutes"),
        ] {
            let error = save_http_check(&path, attempt).unwrap_err();
            assert_eq!(error.code, "invalid_input");
            assert!(error.message.contains(message), "{}", error.message);
        }
        // An invalid URL with a valid interval changes nothing either.
        let error = save_http_check(
            &path,
            HttpCheckInput {
                url: "ftp://example.com/".into(),
                ..supervised(&host, 5, 60)
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "invalid_input");
        assert_eq!(list(&path).unwrap().http_checks, vec![original]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn pausing_keeps_the_interval_and_editing_keeps_the_pause() {
        let path = path();
        let host = resource(&path);
        let shop = application(&path);
        save_http_check(&path, input(&shop, "https://shop.example/")).unwrap();
        let off = set_http_check_paused(&path, &shop, true).unwrap_err();
        assert_eq!(off.code, "invalid_input");
        assert!(
            off.message.contains("Supervision is off"),
            "{}",
            off.message
        );
        let missing = set_http_check_paused(&path, &host, true).unwrap_err();
        assert!(missing.message.contains("No HTTP check is configured"));

        save_http_check(&path, supervised(&host, 10, 60)).unwrap();
        let paused = set_http_check_paused(&path, &host, true).unwrap();
        assert_eq!(
            (paused.monitor.as_str(), paused.monitor_interval_minutes),
            ("paused", Some(10))
        );
        // Editing a paused check never resumes it, even with a new interval.
        let edited = save_http_check(&path, supervised(&host, 15, 60)).unwrap();
        assert_eq!(
            (edited.monitor.as_str(), edited.monitor_interval_minutes),
            ("paused", Some(15))
        );
        let resumed = set_http_check_paused(&path, &host, false).unwrap();
        assert_eq!(resumed.monitor, "active");
        // Choosing no interval turns supervision off; choosing one again starts it active.
        let unsupervised = save_http_check(&path, input(&host, "https://example.com/")).unwrap();
        assert_eq!(
            (
                unsupervised.monitor.as_str(),
                unsupervised.monitor_interval_minutes
            ),
            ("off", None)
        );
        set_http_check_paused(&path, &host, false).unwrap_err();
        assert_eq!(
            save_http_check(&path, supervised(&host, 5, 15))
                .unwrap()
                .monitor,
            "active"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_removed_check_records_no_late_result() {
        let path = path();
        let host = resource(&path);
        save_http_check(&path, input(&host, "https://example.com/")).unwrap();
        remove_http_check(&path, host.clone()).unwrap();
        // What a run that was already in flight would write once its request finished.
        let error = records::store_observation(
            &path,
            NewObservation {
                subject: &host,
                result: "healthy",
                summary: "HTTP 200 as expected",
                observed_at: records::now_ms(),
                valid_for_minutes: 60,
                recorded_at: records::now_ms(),
                http: Some(&HttpEvidence {
                    url: "https://example.com/".into(),
                    expected_status: 200,
                    status: Some(200),
                    latency_ms: 5,
                    error: None,
                    redirects: vec![],
                }),
                lifeline: None,
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "cancelled");
        let snapshot = list(&path).unwrap();
        assert!(snapshot.observations.is_empty());
        assert!(snapshot.events.is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_result_written_after_devhq_closed_is_not_recorded() {
        let path = path();
        let host = resource(&path);
        save_http_check(&path, input(&host, "https://example.com/")).unwrap();
        let evidence = HttpEvidence {
            url: "https://example.com/".into(),
            expected_status: 200,
            status: Some(200),
            latency_ms: 5,
            error: None,
            redirects: vec![],
        };
        let write = |lifeline: &Lifeline| {
            records::store_observation(
                &path,
                NewObservation {
                    subject: &host,
                    result: "healthy",
                    summary: "HTTP 200 as expected",
                    observed_at: records::now_ms(),
                    valid_for_minutes: 60,
                    recorded_at: records::now_ms(),
                    http: Some(&evidence),
                    lifeline: Some(lifeline),
                },
            )
        };
        let lifeline = Lifeline::default();
        write(&lifeline).unwrap();
        // What a run whose request had already ended would write once DevHQ is closing.
        lifeline.end();
        let error = write(&lifeline).unwrap_err();
        assert_eq!(error.code, "cancelled");
        assert!(error.message.contains("DevHQ closed"), "{}", error.message);
        let snapshot = list(&path).unwrap();
        assert_eq!(
            snapshot.observations.len(),
            1,
            "only the one written before"
        );
        assert_eq!(snapshot.events.len(), 1);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn schema_keeps_http_evidence_consistent() {
        let path = path();
        let host = resource(&path);
        let Subject::Resource { id } = &host else {
            unreachable!()
        };
        let db = open(&path).unwrap();
        let observation = |source: &str,
                           result: &str,
                           url: Option<&str>,
                           status: Option<i64>,
                           latency: Option<i64>,
                           error: Option<&str>| {
            db.execute(
                "INSERT INTO observations (id, resource_id, result, summary, source, observed_at, valid_until, recorded_at,
                                           http_url, http_expected_status, http_status, http_latency_ms, http_error, http_redirects)
                 VALUES (?1, ?2, ?3, 'checked', ?4, 10, 20, 10, ?5, CASE WHEN ?5 IS NULL THEN NULL ELSE 200 END, ?6, ?7, ?8,
                         CASE WHEN ?5 IS NULL THEN NULL ELSE '[]' END)",
                params![Uuid::new_v4().to_string(), id, result, source, url, status, latency, error],
            )
        };
        let url = Some("http://x/");
        assert!(
            observation("manual", "healthy", url, Some(200), Some(5), None).is_err(),
            "manual with HTTP evidence"
        );
        assert!(
            observation("http", "healthy", None, None, None, None).is_err(),
            "HTTP without evidence"
        );
        assert!(
            observation("http", "healthy", url, Some(200), None, None).is_err(),
            "HTTP without latency"
        );
        assert!(
            observation("http", "unhealthy", url, None, Some(5), None).is_err(),
            "neither status nor error"
        );
        assert!(
            observation("http", "healthy", url, Some(500), Some(5), None).is_err(),
            "unexpected status is not healthy"
        );
        assert!(
            observation(
                "http",
                "healthy",
                url,
                Some(200),
                Some(5),
                Some("Timed out")
            )
            .is_err(),
            "an error is not healthy"
        );
        assert!(
            observation("http", "unhealthy", url, Some(200), Some(5), None).is_err(),
            "the expected status is not unhealthy"
        );
        assert!(
            observation("http", "degraded", url, Some(500), Some(5), None).is_err(),
            "checks are healthy or unhealthy"
        );
        assert!(observation("http", "unhealthy", url, None, Some(5), Some("Refused")).is_ok());
        assert!(observation("manual", "degraded", None, None, None, None).is_ok());
        assert!(
            db.execute(
                "INSERT INTO events (id, resource_id, kind, outcome, title, source, occurred_at, recorded_at)
                 VALUES ('note', ?1, 'note', 'info', 'Noted', 'http', 1, 1)",
                [id],
            )
            .is_err(),
            "only observation events can come from a check"
        );
        let check = |resource: Option<&str>, url: &str, expected: i64, timeout: i64| {
            db.execute(
                "INSERT INTO http_checks (resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 60, 0)",
                params![resource, url, expected, timeout],
            )
        };
        assert!(check(None, "http://x/", 200, 5).is_err(), "no subject");
        assert!(
            check(Some("ghost"), "http://x/", 200, 5).is_err(),
            "missing resource"
        );
        assert!(check(Some(id), "ftp://x/", 200, 5).is_err(), "scheme");
        assert!(
            check(Some(id), "http://x/", 301, 5).is_err(),
            "3xx expected"
        );
        assert!(check(Some(id), "http://x/", 200, 11).is_err(), "timeout");
        assert!(check(Some(id), "http://x/", 200, 5).is_ok());
        assert!(
            check(Some(id), "http://y/", 200, 5).is_err(),
            "one check per subject"
        );
        assert!(
            db.execute("DELETE FROM resources WHERE id = ?1", [id])
                .is_err(),
            "evidence keeps its subject"
        );
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn configuration_goes_with_its_subject_but_evidence_does_not() {
        let path = path();
        let host = resource(&path);
        let Subject::Resource { id } = &host else {
            unreachable!()
        };
        save_http_check(&path, input(&host, "http://127.0.0.1:9/")).unwrap();
        let db = open(&path).unwrap();
        // What deleting a subject relies on: its configuration goes with it.
        db.execute("DELETE FROM resources WHERE id = ?1", [id])
            .unwrap();
        let remaining: i64 = db
            .query_row("SELECT count(*) FROM http_checks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 0);
        drop(db);
        std::fs::remove_file(path).unwrap();
    }
}
