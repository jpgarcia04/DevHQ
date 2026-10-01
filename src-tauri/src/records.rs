use crate::inventory::{
    allowed, bounded_text, open, required_text, ApiError, MAX_DESCRIPTION, MAX_SUMMARY, MAX_TITLE,
};
use rusqlite::types::Type;
use rusqlite::{params, OptionalExtension, Params, Row, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

const OBSERVATION_RESULTS: &[&str] = &["healthy", "degraded", "unhealthy"];
const SEVERITIES: &[&str] = &["info", "warning", "critical"];
const INCIDENT_STATUSES: &[&str] = &["open", "investigating", "resolved"];
// Observation and incident events are written by their own use cases, never by hand.
const MANUAL_EVENT_KINDS: &[&str] = &["deployment", "backup", "maintenance", "note"];
const EVENT_OUTCOMES: &[&str] = &["success", "warning", "failure", "info"];
// Who produced a record: a person, or DevHQ running an HTTP check the person asked for.
// Incidents and hand-logged events are always manual.
pub(crate) const MANUAL: &str = "manual";
pub(crate) const HTTP: &str = "http";
pub(crate) const MAX_VALIDITY_MINUTES: i64 = 30 * 24 * 60;
// Tolerates small clock differences; anything later is a claim about the future.
const CLOCK_SKEW_MS: i64 = 60_000;

const OBSERVATION_SELECT: &str = "SELECT id, application_id, resource_id, result, summary, source, observed_at, valid_until, recorded_at, http_url, http_expected_status, http_status, http_latency_ms, http_error, http_redirects FROM observations";
const INCIDENT_SELECT: &str = "SELECT id, application_id, resource_id, title, description, severity, status, source, opened_at, updated_at, resolved_at FROM incidents";
const EVENT_SELECT: &str = "SELECT id, application_id, resource_id, kind, outcome, title, detail, source, observation_id, incident_id, occurred_at, recorded_at FROM events";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Subject {
    Application { id: String },
    Resource { id: String },
}

impl Subject {
    pub(crate) fn columns(&self) -> (Option<&str>, Option<&str>) {
        match self {
            Subject::Application { id } => (Some(id), None),
            Subject::Resource { id } => (None, Some(id)),
        }
    }

    /// Which of a pair of per-type statements serves this subject, and its ID. Each statement
    /// of the pair filters one subject column, so that it follows that column's index.
    pub(crate) fn per_type(&self) -> (usize, &str) {
        match self {
            Subject::Application { id } => (0, id),
            Subject::Resource { id } => (1, id),
        }
    }

    pub(crate) fn label(&self) -> &'static str {
        match self {
            Subject::Application { .. } => "application",
            Subject::Resource { .. } => "resource",
        }
    }

    pub(crate) fn from_columns(
        application_id: Option<String>,
        resource_id: Option<String>,
    ) -> rusqlite::Result<Self> {
        match (application_id, resource_id) {
            (Some(id), None) => Ok(Subject::Application { id }),
            (None, Some(id)) => Ok(Subject::Resource { id }),
            _ => Err(rusqlite::Error::FromSqlConversionFailure(
                1,
                Type::Null,
                "a record must reference exactly one application or resource".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub id: String,
    pub subject: Subject,
    pub result: String,
    pub summary: String,
    pub source: String,
    pub observed_at: String,
    pub valid_until: String,
    pub recorded_at: String,
    /// Technical evidence of an HTTP check; absent for manual entries.
    pub http: Option<HttpEvidence>,
}

/// What an HTTP check requested and what came back. `status` is the last status received
/// (absent when no response arrived); `error` explains any failure of the checked system.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HttpEvidence {
    pub url: String,
    pub expected_status: i64,
    pub status: Option<i64>,
    pub latency_ms: i64,
    pub error: Option<String>,
    pub redirects: Vec<Redirect>,
}

/// One redirect that was followed: the status that sent it and where it pointed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Redirect {
    pub status: i64,
    pub location: String,
}

impl HttpEvidence {
    /// Healthy exactly when the expected status arrived without error. The schema enforces it too.
    pub(crate) fn result(&self) -> &'static str {
        if self.error.is_none() && self.status == Some(self.expected_status) {
            "healthy"
        } else {
            "unhealthy"
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationInput {
    pub subject: Subject,
    pub result: String,
    pub summary: String,
    pub observed_at: Option<String>,
    pub valid_for_minutes: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Incident {
    pub id: String,
    pub subject: Subject,
    pub title: String,
    pub description: String,
    pub severity: String,
    pub status: String,
    pub source: String,
    pub opened_at: String,
    pub updated_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncidentInput {
    pub subject: Subject,
    pub title: String,
    pub description: String,
    pub severity: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncidentStatusInput {
    pub id: String,
    pub status: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: String,
    pub subject: Subject,
    pub kind: String,
    pub outcome: String,
    pub title: String,
    pub detail: String,
    pub source: String,
    pub observation_id: Option<String>,
    pub incident_id: Option<String>,
    pub occurred_at: String,
    pub recorded_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventInput {
    pub subject: Subject,
    pub kind: String,
    pub outcome: String,
    pub title: String,
    pub detail: String,
    pub occurred_at: Option<String>,
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

fn parse_time(value: &str, label: &str) -> Result<i64, ApiError> {
    let parsed = OffsetDateTime::parse(value, &Rfc3339).map_err(|_| {
        ApiError::invalid(format!(
            "{label} must be an RFC 3339 timestamp with a time zone."
        ))
    })?;
    i64::try_from(parsed.unix_timestamp_nanos() / 1_000_000)
        .map_err(|_| ApiError::invalid(format!("{label} is out of range.")))
}

/// A person can report what already happened, not what has not happened yet.
fn reported_time(value: &Option<String>, now: i64, label: &str) -> Result<i64, ApiError> {
    let Some(value) = value else { return Ok(now) };
    let time = parse_time(value, label)?;
    if time > now + CLOCK_SKEW_MS {
        return Err(ApiError::invalid(format!(
            "{label} cannot be in the future."
        )));
    }
    Ok(time)
}

pub(crate) fn format_time(ms: i64) -> Option<String> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
        .ok()?
        .format(&Rfc3339)
        .ok()
}

pub(crate) fn time_at(row: &Row, index: usize) -> rusqlite::Result<String> {
    let ms: i64 = row.get(index)?;
    format_time(ms).ok_or(rusqlite::Error::IntegralValueOutOfRange(index, ms))
}

fn optional_time_at(row: &Row, index: usize) -> rusqlite::Result<Option<String>> {
    match row.get::<_, Option<i64>>(index)? {
        Some(_) => time_at(row, index).map(Some),
        None => Ok(None),
    }
}

fn observation_from_row(row: &Row) -> rusqlite::Result<Observation> {
    let http = match row.get::<_, Option<String>>(9)? {
        Some(url) => {
            let redirects: String = row.get(14)?;
            Some(HttpEvidence {
                url,
                expected_status: row.get(10)?,
                status: row.get(11)?,
                latency_ms: row.get(12)?,
                error: row.get(13)?,
                redirects: serde_json::from_str(&redirects).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(14, Type::Text, Box::new(error))
                })?,
            })
        }
        None => None,
    };
    Ok(Observation {
        id: row.get(0)?,
        subject: Subject::from_columns(row.get(1)?, row.get(2)?)?,
        result: row.get(3)?,
        summary: row.get(4)?,
        source: row.get(5)?,
        observed_at: time_at(row, 6)?,
        valid_until: time_at(row, 7)?,
        recorded_at: time_at(row, 8)?,
        http,
    })
}

fn incident_from_row(row: &Row) -> rusqlite::Result<Incident> {
    Ok(Incident {
        id: row.get(0)?,
        subject: Subject::from_columns(row.get(1)?, row.get(2)?)?,
        title: row.get(3)?,
        description: row.get(4)?,
        severity: row.get(5)?,
        status: row.get(6)?,
        source: row.get(7)?,
        opened_at: time_at(row, 8)?,
        updated_at: time_at(row, 9)?,
        resolved_at: optional_time_at(row, 10)?,
    })
}

fn event_from_row(row: &Row) -> rusqlite::Result<Event> {
    Ok(Event {
        id: row.get(0)?,
        subject: Subject::from_columns(row.get(1)?, row.get(2)?)?,
        kind: row.get(3)?,
        outcome: row.get(4)?,
        title: row.get(5)?,
        detail: row.get(6)?,
        source: row.get(7)?,
        observation_id: row.get(8)?,
        incident_id: row.get(9)?,
        occurred_at: time_at(row, 10)?,
        recorded_at: time_at(row, 11)?,
    })
}

fn query_all<T>(
    tx: &Transaction,
    sql: &str,
    params: impl Params,
    map: fn(&Row) -> rusqlite::Result<T>,
) -> Result<Vec<T>, ApiError> {
    let mut statement = tx.prepare(sql).map_err(ApiError::database)?;
    let rows = statement
        .query_map(params, map)
        .map_err(ApiError::database)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(ApiError::database);
    rows
}

fn query_one<T>(
    tx: &Transaction,
    select: &str,
    id: &str,
    map: fn(&Row) -> rusqlite::Result<T>,
) -> Result<Option<T>, ApiError> {
    tx.query_row(&format!("{select} WHERE id = ?1"), [id], map)
        .optional()
        .map_err(ApiError::database)
}

pub(crate) struct Records {
    pub observations: Vec<Observation>,
    pub incidents: Vec<Incident>,
    pub events: Vec<Event>,
    pub automated_history: AutomatedHistory,
}

/// Automated results a snapshot carries per application or resource. Manual records grow as
/// fast as a person types and are always loaded in full; supervision can add a result every
/// 5 minutes for as long as DevHQ is open, so a snapshot carries only the latest ones. Older
/// results stay in the database, untouched.
pub(crate) const AUTOMATED_HISTORY_LIMIT: i64 = 200;

/// What a snapshot leaves out, so that the window can say so instead of implying there is
/// nothing older.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutomatedHistory {
    pub limit: i64,
    /// Subjects with older automated results than the snapshot carries.
    pub truncated: Vec<TruncatedHistory>,
}

/// From `since` on, the subject's automated observations and their events are complete;
/// before it, only its manual records are loaded.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TruncatedHistory {
    pub subject: Subject,
    pub since: String,
}

// A subject's latest automated observations: one statement per subject type, so that each
// follows its own subject index (see Subject::per_type).
pub(crate) const LATEST_AUTOMATED: [&str; 2] = [
    "SELECT rowid, observed_at FROM observations WHERE application_id = ?1 AND source = 'http'
     ORDER BY observed_at DESC, recorded_at DESC, rowid DESC LIMIT ?2",
    "SELECT rowid, observed_at FROM observations WHERE resource_id = ?1 AND source = 'http'
     ORDER BY observed_at DESC, recorded_at DESC, rowid DESC LIMIT ?2",
];
// Every manual record, plus the automated observations chosen above (a JSON array of rowids)
// and exactly their events.
const WINDOWED_OBSERVATIONS: &str = "WHERE rowid IN (
       SELECT rowid FROM observations WHERE source = 'manual'
       UNION ALL SELECT value FROM json_each(?1))
     ORDER BY observed_at DESC, recorded_at DESC, rowid DESC";
const WINDOWED_EVENTS: &str = "WHERE rowid IN (
       SELECT rowid FROM events WHERE source = 'manual'
       UNION ALL SELECT e.rowid FROM json_each(?1) AS w
         JOIN observations AS o ON o.rowid = w.value
         JOIN events AS e ON e.observation_id = o.id)
     ORDER BY occurred_at DESC, recorded_at DESC, rowid DESC";

/// Newest first. Reads inside the caller's transaction so the snapshot is consistent. Every
/// read follows an index, so its cost grows with what is loaded, not with what is stored.
pub(crate) fn load(tx: &Transaction, subjects: &[Subject]) -> Result<Records, ApiError> {
    let prepare = |sql| tx.prepare(sql).map_err(ApiError::database);
    let mut latest = [prepare(LATEST_AUTOMATED[0])?, prepare(LATEST_AUTOMATED[1])?];
    let limit = usize::try_from(AUTOMATED_HISTORY_LIMIT).map_err(ApiError::database)?;
    let mut observation_rows = Vec::new();
    let mut truncated = Vec::new();
    for subject in subjects {
        let (kind, id) = subject.per_type();
        // One more than the limit tells whether anything older exists.
        let mut rows = latest[kind]
            .query_map(params![id, AUTOMATED_HISTORY_LIMIT + 1], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(ApiError::database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(ApiError::database)?;
        if rows.len() > limit {
            rows.truncate(limit);
            if let Some(&(_, oldest)) = rows.last() {
                truncated.push(TruncatedHistory {
                    subject: subject.clone(),
                    since: format_time(oldest)
                        .ok_or_else(|| ApiError::database("an observation time is out of range"))?,
                });
            }
        }
        observation_rows.extend(rows.into_iter().map(|(rowid, _)| rowid));
    }
    let window = serde_json::to_string(&observation_rows).map_err(ApiError::database)?;
    Ok(Records {
        observations: query_all(
            tx,
            &format!("{OBSERVATION_SELECT} {WINDOWED_OBSERVATIONS}"),
            [&window],
            observation_from_row,
        )?,
        incidents: query_all(
            tx,
            &format!("{INCIDENT_SELECT} ORDER BY opened_at DESC, rowid DESC"),
            [],
            incident_from_row,
        )?,
        // Each automated observation wrote its event with it, so the same rows choose them.
        events: query_all(
            tx,
            &format!("{EVENT_SELECT} {WINDOWED_EVENTS}"),
            [&window],
            event_from_row,
        )?,
        automated_history: AutomatedHistory {
            limit: AUTOMATED_HISTORY_LIMIT,
            truncated,
        },
    })
}

pub(crate) fn ensure_subject(tx: &Transaction, subject: &Subject) -> Result<(), ApiError> {
    let (sql, id, label) = match subject {
        Subject::Application { id } => (
            "SELECT EXISTS(SELECT 1 FROM applications WHERE id = ?1)",
            id,
            "application",
        ),
        Subject::Resource { id } => (
            "SELECT EXISTS(SELECT 1 FROM resources WHERE id = ?1)",
            id,
            "resource",
        ),
    };
    let exists: bool = tx
        .query_row(sql, [id], |row| row.get(0))
        .map_err(ApiError::database)?;
    if exists {
        Ok(())
    } else {
        Err(ApiError::invalid(format!(
            "The {label} {id} does not exist. Choose an existing {label}."
        )))
    }
}

struct NewEvent<'a> {
    subject: &'a Subject,
    source: &'a str,
    kind: &'a str,
    outcome: &'a str,
    title: String,
    detail: &'a str,
    observation_id: Option<&'a str>,
    incident_id: Option<&'a str>,
    occurred_at: i64,
    recorded_at: i64,
}

fn insert_event(tx: &Transaction, event: NewEvent) -> Result<String, ApiError> {
    let id = Uuid::new_v4().to_string();
    let (application_id, resource_id) = event.subject.columns();
    tx.execute(
        "INSERT INTO events (id, application_id, resource_id, kind, outcome, title, detail, source, observation_id, incident_id, occurred_at, recorded_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![id, application_id, resource_id, event.kind, event.outcome, event.title, event.detail, event.source, event.observation_id, event.incident_id, event.occurred_at, event.recorded_at],
    )
    .map_err(ApiError::database)?;
    Ok(id)
}

fn result_label(result: &str) -> &'static str {
    match result {
        "healthy" => "Healthy",
        "degraded" => "Attention required",
        _ => "Unhealthy",
    }
}

fn result_outcome(result: &str) -> &'static str {
    match result {
        "healthy" => "success",
        "degraded" => "warning",
        _ => "failure",
    }
}

fn severity_outcome(severity: &str) -> &'static str {
    match severity {
        "critical" => "failure",
        "warning" => "warning",
        _ => "info",
    }
}

/// An observation ready to store. Both sources go through the same path, so every
/// observation gets its timeline event in the same transaction.
pub(crate) struct NewObservation<'a> {
    pub subject: &'a Subject,
    pub result: &'a str,
    pub summary: &'a str,
    pub observed_at: i64,
    pub valid_for_minutes: i64,
    pub recorded_at: i64,
    pub http: Option<&'a HttpEvidence>,
}

pub(crate) fn store_observation(path: &Path, new: NewObservation) -> Result<Observation, ApiError> {
    let valid_until = new.observed_at + new.valid_for_minutes * 60_000;
    let (source, title) = match new.http {
        Some(_) => (HTTP, "HTTP check"),
        None => (MANUAL, "Observation"),
    };
    let redirects = new
        .http
        .map(|http| serde_json::to_string(&http.redirects))
        .transpose()
        .map_err(ApiError::database)?;
    let mut db = open(path)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(ApiError::database)?;
    ensure_subject(&tx, new.subject)?;
    let (application_id, resource_id) = new.subject.columns();
    // Removing a check cancels its run in flight. If the removal commits first, this write is
    // refused; SQLite orders the two, so a removed check never records a late result.
    if new.http.is_some() {
        let configured: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM http_checks WHERE application_id IS ?1 AND resource_id IS ?2)",
                params![application_id, resource_id],
                |row| row.get(0),
            )
            .map_err(ApiError::database)?;
        if !configured {
            return Err(ApiError::cancelled(
                "The HTTP check was removed while it ran. Nothing was recorded.",
            ));
        }
    }
    let id = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO observations (id, application_id, resource_id, result, summary, source, observed_at, valid_until, recorded_at,
                                   http_url, http_expected_status, http_status, http_latency_ms, http_error, http_redirects)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            id,
            application_id,
            resource_id,
            new.result,
            new.summary,
            source,
            new.observed_at,
            valid_until,
            new.recorded_at,
            new.http.map(|http| &http.url),
            new.http.map(|http| http.expected_status),
            new.http.and_then(|http| http.status),
            new.http.map(|http| http.latency_ms),
            new.http.and_then(|http| http.error.as_ref()),
            redirects,
        ],
    )
    .map_err(ApiError::database)?;
    insert_event(
        &tx,
        NewEvent {
            subject: new.subject,
            source,
            kind: "observation",
            outcome: result_outcome(new.result),
            title: format!("{title}: {}", result_label(new.result)),
            detail: new.summary,
            observation_id: Some(&id),
            incident_id: None,
            occurred_at: new.observed_at,
            recorded_at: new.recorded_at,
        },
    )?;
    let observation = query_one(&tx, OBSERVATION_SELECT, &id, observation_from_row)?
        .ok_or_else(|| ApiError::database("Observation was not saved."))?;
    tx.commit().map_err(ApiError::database)?;
    Ok(observation)
}

// A subject's automated observations older than a cursor, newest first. The plain bound on
// observed_at lets the subject index find the start; the row value breaks ties exactly.
const OLDER_AUTOMATED: [&str; 2] = [
    "WHERE application_id = ?1 AND source = 'http' AND observed_at <= ?2
       AND (observed_at, recorded_at, rowid) < (?2, ?3, ?4)
     ORDER BY observed_at DESC, recorded_at DESC, rowid DESC LIMIT ?5",
    "WHERE resource_id = ?1 AND source = 'http' AND observed_at <= ?2
       AND (observed_at, recorded_at, rowid) < (?2, ?3, ?4)
     ORDER BY observed_at DESC, recorded_at DESC, rowid DESC LIMIT ?5",
];

/// Automated observations a snapshot left out, a page at a time.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationPage {
    pub observations: Vec<Observation>,
    /// Older automated observations remain after this page.
    pub more: bool,
}

/// The subject's automated observations older than `before`, one of its observations already
/// shown, newest first and at most `AUTOMATED_HISTORY_LIMIT` of them. Anchoring on a shown
/// observation, rather than counting, keeps pages exact while new results arrive.
pub fn older_observations(
    path: &Path,
    subject: &Subject,
    before: &str,
) -> Result<ObservationPage, ApiError> {
    let mut db = open(path)?;
    let tx = db.transaction().map_err(ApiError::database)?;
    ensure_subject(&tx, subject)?;
    let (application_id, resource_id) = subject.columns();
    let cursor: Option<(i64, i64, i64)> = tx
        .query_row(
            "SELECT observed_at, recorded_at, rowid FROM observations
             WHERE id = ?1 AND application_id IS ?2 AND resource_id IS ?3",
            params![before, application_id, resource_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(ApiError::database)?;
    let Some((observed_at, recorded_at, rowid)) = cursor else {
        return Err(ApiError::invalid(format!(
            "Observation {before} is not one of this {}'s.",
            subject.label()
        )));
    };
    let (kind, id) = subject.per_type();
    let mut observations = {
        let mut statement = tx
            .prepare(&format!("{OBSERVATION_SELECT} {}", OLDER_AUTOMATED[kind]))
            .map_err(ApiError::database)?;
        let rows = statement
            .query_map(
                params![
                    id,
                    observed_at,
                    recorded_at,
                    rowid,
                    AUTOMATED_HISTORY_LIMIT + 1
                ],
                observation_from_row,
            )
            .map_err(ApiError::database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(ApiError::database)?;
        rows
    };
    let limit = usize::try_from(AUTOMATED_HISTORY_LIMIT).map_err(ApiError::database)?;
    let more = observations.len() > limit;
    observations.truncate(limit);
    tx.commit().map_err(ApiError::database)?;
    Ok(ObservationPage { observations, more })
}

// A subject's automated events older than a position in the timeline, newest first: only
// their positions, which merge across subjects. As for observations, the plain bound on
// occurred_at lets the subject index find the start (current SQLite also derives it from the
// row value, but the plan does not depend on that); the row value breaks ties exactly.
const OLDER_AUTOMATED_EVENTS: [&str; 2] = [
    "SELECT occurred_at, recorded_at, rowid FROM events
     WHERE application_id = ?1 AND source = 'http' AND occurred_at <= ?2
       AND (occurred_at, recorded_at, rowid) < (?2, ?3, ?4)
     ORDER BY occurred_at DESC, recorded_at DESC, rowid DESC LIMIT ?5",
    "SELECT occurred_at, recorded_at, rowid FROM events
     WHERE resource_id = ?1 AND source = 'http' AND occurred_at <= ?2
       AND (occurred_at, recorded_at, rowid) < (?2, ?3, ?4)
     ORDER BY occurred_at DESC, recorded_at DESC, rowid DESC LIMIT ?5",
];
// The events chosen above (a JSON array of rowids), in timeline order.
const EVENTS_BY_ROWID: &str = "WHERE rowid IN (SELECT value FROM json_each(?1))
     ORDER BY occurred_at DESC, recorded_at DESC, rowid DESC";

/// Automated events a snapshot left out of a timeline, a page at a time.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventPage {
    pub events: Vec<Event>,
    /// Older automated events of these subjects remain after this page.
    pub more: bool,
}

/// The automated events of every given subject older than `before`, one of their events already
/// shown, newest first across all of them and at most `AUTOMATED_HISTORY_LIMIT` in total. A
/// timeline that merges several systems pages through them together: after a page it is
/// complete down to the page's oldest event, so pages never leave a gap between systems.
/// Anchoring on a shown event, rather than counting, keeps pages exact while new results
/// arrive. Each subject's read follows its own index, so a page costs what it returns per
/// subject, never the whole automated history.
pub fn older_events(
    path: &Path,
    subjects: &[Subject],
    before: &str,
) -> Result<EventPage, ApiError> {
    let mut subjects = subjects.to_vec();
    subjects.sort_by(|a, b| a.per_type().cmp(&b.per_type()));
    subjects.dedup();
    if subjects.is_empty() {
        return Err(ApiError::invalid(
            "Choose at least one application or resource.",
        ));
    }
    let mut db = open(path)?;
    let tx = db.transaction().map_err(ApiError::database)?;
    for subject in &subjects {
        ensure_subject(&tx, subject)?;
    }
    let cursor = tx
        .query_row(
            "SELECT application_id, resource_id, occurred_at, recorded_at, rowid FROM events WHERE id = ?1",
            [before],
            |row| {
                Ok((
                    Subject::from_columns(row.get(0)?, row.get(1)?)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()
        .map_err(ApiError::database)?;
    // The anchor must be one of these subjects' own events.
    let Some((_, occurred_at, recorded_at, rowid)) =
        cursor.filter(|(owner, ..)| subjects.contains(owner))
    else {
        return Err(ApiError::invalid(format!(
            "Event {before} is not one of these systems' events."
        )));
    };
    // One more than the page from each subject tells whether anything older remains.
    let mut positions = Vec::new();
    {
        let prepare = |sql| tx.prepare(sql).map_err(ApiError::database);
        let mut older = [
            prepare(OLDER_AUTOMATED_EVENTS[0])?,
            prepare(OLDER_AUTOMATED_EVENTS[1])?,
        ];
        for subject in &subjects {
            let (kind, id) = subject.per_type();
            let rows = older[kind]
                .query_map(
                    params![
                        id,
                        occurred_at,
                        recorded_at,
                        rowid,
                        AUTOMATED_HISTORY_LIMIT + 1
                    ],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    },
                )
                .map_err(ApiError::database)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(ApiError::database)?;
            positions.extend(rows);
        }
    }
    // Newest first across subjects, exactly as the timeline orders them.
    positions.sort_unstable_by(|a, b| b.cmp(a));
    let limit = usize::try_from(AUTOMATED_HISTORY_LIMIT).map_err(ApiError::database)?;
    let more = positions.len() > limit;
    positions.truncate(limit);
    let chosen: Vec<i64> = positions.iter().map(|&(_, _, rowid)| rowid).collect();
    let events = query_all(
        &tx,
        &format!("{EVENT_SELECT} {EVENTS_BY_ROWID}"),
        [serde_json::to_string(&chosen).map_err(ApiError::database)?],
        event_from_row,
    )?;
    tx.commit().map_err(ApiError::database)?;
    Ok(EventPage { events, more })
}

/// Stores a manual observation and its timeline event atomically.
pub fn record_observation(path: &Path, input: ObservationInput) -> Result<Observation, ApiError> {
    allowed(&input.result, OBSERVATION_RESULTS, "observation result")?;
    let summary = required_text(&input.summary, MAX_SUMMARY, "Observation summary")?;
    if !(1..=MAX_VALIDITY_MINUTES).contains(&input.valid_for_minutes) {
        return Err(ApiError::invalid(
            "Validity must be between 1 minute and 30 days.",
        ));
    }
    let recorded_at = now_ms();
    let observed_at = reported_time(&input.observed_at, recorded_at, "Observed time")?;
    store_observation(
        path,
        NewObservation {
            subject: &input.subject,
            result: &input.result,
            summary: &summary,
            observed_at,
            valid_for_minutes: input.valid_for_minutes,
            recorded_at,
            http: None,
        },
    )
}

/// Opens a manual incident and records the opening in the timeline atomically.
pub fn open_incident(path: &Path, input: IncidentInput) -> Result<Incident, ApiError> {
    let title = required_text(&input.title, MAX_TITLE, "Incident title")?;
    allowed(&input.severity, SEVERITIES, "incident severity")?;
    let description = bounded_text(&input.description, MAX_DESCRIPTION, "Incident description")?;
    let now = now_ms();
    let mut db = open(path)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(ApiError::database)?;
    ensure_subject(&tx, &input.subject)?;
    let id = Uuid::new_v4().to_string();
    let (application_id, resource_id) = input.subject.columns();
    tx.execute(
        "INSERT INTO incidents (id, application_id, resource_id, title, description, severity, status, source, opened_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'open', ?7, ?8, ?8)",
        params![id, application_id, resource_id, title, description, input.severity, MANUAL, now],
    )
    .map_err(ApiError::database)?;
    insert_event(
        &tx,
        NewEvent {
            subject: &input.subject,
            source: MANUAL,
            kind: "incident",
            outcome: severity_outcome(&input.severity),
            title: format!("Incident opened: {title}"),
            detail: &description,
            observation_id: None,
            incident_id: Some(&id),
            occurred_at: now,
            recorded_at: now,
        },
    )?;
    let incident = query_one(&tx, INCIDENT_SELECT, &id, incident_from_row)?
        .ok_or_else(|| ApiError::database("Incident was not saved."))?;
    tx.commit().map_err(ApiError::database)?;
    Ok(incident)
}

/// Moves an incident to another status and keeps the change, with its note, in the timeline.
pub fn update_incident_status(
    path: &Path,
    input: IncidentStatusInput,
) -> Result<Incident, ApiError> {
    allowed(&input.status, INCIDENT_STATUSES, "incident status")?;
    let note = bounded_text(&input.note, MAX_SUMMARY, "Status note")?;
    let now = now_ms();
    let mut db = open(path)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(ApiError::database)?;
    let current = query_one(&tx, INCIDENT_SELECT, &input.id, incident_from_row)?
        .ok_or_else(|| ApiError::invalid(format!("Incident {} does not exist.", input.id)))?;
    if current.status == input.status {
        return Err(ApiError::invalid(format!(
            "The incident is already {}.",
            input.status
        )));
    }
    let resolved_at = (input.status == "resolved").then_some(now);
    tx.execute(
        "UPDATE incidents SET status = ?2, updated_at = ?3, resolved_at = ?4 WHERE id = ?1",
        params![input.id, input.status, now, resolved_at],
    )
    .map_err(ApiError::database)?;
    let (change, outcome) = match input.status.as_str() {
        "resolved" => ("resolved", "success"),
        "investigating" => ("under investigation", "info"),
        _ => ("reopened", severity_outcome(&current.severity)),
    };
    insert_event(
        &tx,
        NewEvent {
            subject: &current.subject,
            source: MANUAL,
            kind: "incident",
            outcome,
            title: format!("Incident {change}: {}", current.title),
            detail: &note,
            observation_id: None,
            incident_id: Some(&input.id),
            occurred_at: now,
            recorded_at: now,
        },
    )?;
    let incident = query_one(&tx, INCIDENT_SELECT, &input.id, incident_from_row)?
        .ok_or_else(|| ApiError::database("Incident was not saved."))?;
    tx.commit().map_err(ApiError::database)?;
    Ok(incident)
}

/// Logs something that happened to an application or resource, as reported by a person.
pub fn record_event(path: &Path, input: EventInput) -> Result<Event, ApiError> {
    allowed(&input.kind, MANUAL_EVENT_KINDS, "event kind")?;
    allowed(&input.outcome, EVENT_OUTCOMES, "event outcome")?;
    let title = required_text(&input.title, MAX_TITLE, "Event title")?;
    let detail = bounded_text(&input.detail, MAX_DESCRIPTION, "Event details")?;
    let recorded_at = now_ms();
    let occurred_at = reported_time(&input.occurred_at, recorded_at, "Event time")?;
    let mut db = open(path)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(ApiError::database)?;
    ensure_subject(&tx, &input.subject)?;
    let id = insert_event(
        &tx,
        NewEvent {
            subject: &input.subject,
            source: MANUAL,
            kind: &input.kind,
            outcome: &input.outcome,
            title,
            detail: &detail,
            observation_id: None,
            incident_id: None,
            occurred_at,
            recorded_at,
        },
    )?;
    let event = query_one(&tx, EVENT_SELECT, &id, event_from_row)?
        .ok_or_else(|| ApiError::database("Event was not saved."))?;
    tx.commit().map_err(ApiError::database)?;
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{
        list, save_application, save_resource, ApplicationInput, ComponentInput, ResourceInput,
    };

    fn path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("devhq-records-test-{}.sqlite3", Uuid::new_v4()))
    }

    /// An application whose API component runs on a resource. Returns (application, resource) IDs.
    fn seed(path: &Path) -> (String, String) {
        let resource = save_resource(
            path,
            ResourceInput {
                id: None,
                name: "Host".into(),
                kind: "server".into(),
                description: String::new(),
                operating_system: "Linux".into(),
            },
        )
        .unwrap();
        let application = save_application(
            path,
            ApplicationInput {
                id: None,
                name: "Shop".into(),
                description: String::new(),
                lifecycle: "production".into(),
                version: None,
                components: vec![ComponentInput {
                    id: None,
                    name: "API".into(),
                    kind: "backend".into(),
                    resource_id: Some(resource.id.clone()),
                }],
            },
        )
        .unwrap();
        (application.id, resource.id)
    }

    fn observation(subject: Subject) -> ObservationInput {
        ObservationInput {
            subject,
            result: "healthy".into(),
            summary: "Checked by hand".into(),
            observed_at: None,
            valid_for_minutes: 60,
        }
    }

    fn incident(subject: Subject) -> IncidentInput {
        IncidentInput {
            subject,
            title: "Checkout failing".into(),
            description: "502 on /pay".into(),
            severity: "critical".into(),
        }
    }

    fn event(subject: Subject) -> EventInput {
        EventInput {
            subject,
            kind: "deployment".into(),
            outcome: "success".into(),
            title: "Deployed 1.5".into(),
            detail: String::new(),
            occurred_at: None,
        }
    }

    fn assert_no_records(path: &Path) {
        let snapshot = list(path).unwrap();
        assert!(snapshot.observations.is_empty());
        assert!(snapshot.incidents.is_empty());
        assert!(snapshot.events.is_empty());
    }

    #[test]
    fn records_persist_with_context_across_reopen() {
        let path = path();
        let (application_id, resource_id) = seed(&path);
        let application = Subject::Application {
            id: application_id.clone(),
        };
        let resource = Subject::Resource {
            id: resource_id.clone(),
        };
        let observed = record_observation(
            &path,
            ObservationInput {
                result: "unhealthy".into(),
                summary: "  Disk at 97%  ".into(),
                observed_at: Some("2025-03-01T11:15:30+01:00".into()),
                valid_for_minutes: 90,
                ..observation(resource.clone())
            },
        )
        .unwrap();
        assert_eq!(observed.observed_at, "2025-03-01T10:15:30Z");
        assert_eq!(observed.valid_until, "2025-03-01T11:45:30Z");
        assert_eq!(observed.summary, "Disk at 97%");
        assert_eq!(observed.source, "manual");
        let opened = open_incident(&path, incident(application.clone())).unwrap();
        assert_eq!((opened.status.as_str(), opened.resolved_at), ("open", None));
        update_incident_status(
            &path,
            IncidentStatusInput {
                id: opened.id.clone(),
                status: "investigating".into(),
                note: String::new(),
            },
        )
        .unwrap();
        let resolved = update_incident_status(
            &path,
            IncidentStatusInput {
                id: opened.id.clone(),
                status: "resolved".into(),
                note: " Rolled back to 1.4 ".into(),
            },
        )
        .unwrap();
        assert!(resolved.resolved_at.is_some());
        let logged = record_event(&path, event(application.clone())).unwrap();

        // Every call opened and closed its own connection; this read is a fresh reopen.
        let reopened = list(&path).unwrap();
        assert_eq!(reopened.observations, vec![observed.clone()]);
        assert_eq!(reopened.incidents, vec![resolved]);
        assert_eq!(reopened.events.len(), 5);
        assert_eq!(reopened.events[0], logged);
        let observation_event = reopened.events.last().unwrap();
        assert_eq!(observation_event.subject, resource);
        assert_eq!(observation_event.kind, "observation");
        assert_eq!(observation_event.outcome, "failure");
        assert_eq!(observation_event.detail, "Disk at 97%");
        assert_eq!(
            observation_event.observation_id.as_deref(),
            Some(observed.id.as_str())
        );
        assert_eq!(observation_event.occurred_at, observed.observed_at);
        let incident_events: Vec<_> = reopened
            .events
            .iter()
            .filter(|item| item.incident_id.as_deref() == Some(opened.id.as_str()))
            .collect();
        assert_eq!(
            incident_events
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            [
                "Incident resolved: Checkout failing",
                "Incident under investigation: Checkout failing",
                "Incident opened: Checkout failing",
            ]
        );
        assert_eq!(incident_events[0].detail, "Rolled back to 1.4");
        assert!(incident_events
            .iter()
            .all(|item| item.subject == application));
        assert!(reopened.events.iter().all(|item| item.source == "manual"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_invalid_references_without_partial_writes() {
        let path = path();
        let (application_id, resource_id) = seed(&path);
        let missing = Subject::Resource { id: "ghost".into() };
        let error = record_observation(&path, observation(missing.clone())).unwrap_err();
        assert_eq!(error.code, "invalid_input");
        assert!(error.message.contains("ghost"));
        let missing_application = Subject::Application { id: "ghost".into() };
        for error in [
            open_incident(&path, incident(missing_application.clone())).unwrap_err(),
            record_event(&path, event(missing)).unwrap_err(),
            // The subject type matters: an application ID is not a resource.
            record_observation(&path, observation(Subject::Resource { id: application_id }))
                .unwrap_err(),
            record_event(&path, event(Subject::Application { id: resource_id })).unwrap_err(),
            update_incident_status(
                &path,
                IncidentStatusInput {
                    id: "ghost".into(),
                    status: "resolved".into(),
                    note: String::new(),
                },
            )
            .unwrap_err(),
        ] {
            assert_eq!(error.code, "invalid_input", "{}", error.message);
        }
        assert_no_records(&path);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_invalid_values_before_writing() {
        let path = path();
        let (application_id, _) = seed(&path);
        let application = Subject::Application { id: application_id };
        let future = format_time(now_ms() + 10 * 60_000).unwrap();
        let invalid_observations = [
            ObservationInput {
                observed_at: Some(future.clone()),
                ..observation(application.clone())
            },
            ObservationInput {
                observed_at: Some("yesterday".into()),
                ..observation(application.clone())
            },
            ObservationInput {
                valid_for_minutes: 0,
                ..observation(application.clone())
            },
            ObservationInput {
                valid_for_minutes: MAX_VALIDITY_MINUTES + 1,
                ..observation(application.clone())
            },
            ObservationInput {
                result: "fine".into(),
                ..observation(application.clone())
            },
            ObservationInput {
                summary: "   ".into(),
                ..observation(application.clone())
            },
        ];
        for input in invalid_observations {
            assert_eq!(
                record_observation(&path, input).unwrap_err().code,
                "invalid_input"
            );
        }
        for input in [
            EventInput {
                kind: "observation".into(),
                ..event(application.clone())
            },
            EventInput {
                outcome: "great".into(),
                ..event(application.clone())
            },
            EventInput {
                occurred_at: Some(future),
                ..event(application.clone())
            },
        ] {
            assert_eq!(
                record_event(&path, input).unwrap_err().code,
                "invalid_input"
            );
        }
        for input in [
            IncidentInput {
                severity: "urgent".into(),
                ..incident(application.clone())
            },
            IncidentInput {
                title: " ".into(),
                ..incident(application.clone())
            },
        ] {
            assert_eq!(
                open_incident(&path, input).unwrap_err().code,
                "invalid_input"
            );
        }
        // Longer than the window lets anyone type: refused, never truncated.
        let long = |count: usize| "x".repeat(count);
        assert!(record_observation(
            &path,
            ObservationInput {
                summary: long(501),
                ..observation(application.clone())
            },
        )
        .unwrap_err()
        .message
        .contains("Observation summary is longer than 500"));
        for input in [
            IncidentInput {
                title: long(161),
                ..incident(application.clone())
            },
            IncidentInput {
                description: long(1001),
                ..incident(application.clone())
            },
        ] {
            assert_eq!(
                open_incident(&path, input).unwrap_err().code,
                "invalid_input"
            );
        }
        for input in [
            EventInput {
                title: long(161),
                ..event(application.clone())
            },
            EventInput {
                detail: long(1001),
                ..event(application.clone())
            },
        ] {
            assert_eq!(
                record_event(&path, input).unwrap_err().code,
                "invalid_input"
            );
        }
        assert_no_records(&path);
        let opened = open_incident(&path, incident(application)).unwrap();
        let long_note = update_incident_status(
            &path,
            IncidentStatusInput {
                id: opened.id.clone(),
                status: "resolved".into(),
                note: long(501),
            },
        )
        .unwrap_err();
        assert!(long_note.message.contains("Status note is longer than 500"));
        let unchanged = update_incident_status(
            &path,
            IncidentStatusInput {
                id: opened.id,
                status: "open".into(),
                note: String::new(),
            },
        )
        .unwrap_err();
        assert_eq!(unchanged.code, "invalid_input");
        let snapshot = list(&path).unwrap();
        assert_eq!(snapshot.incidents[0].status, "open");
        assert_eq!(
            snapshot.incidents[0].updated_at,
            snapshot.incidents[0].opened_at
        );
        assert_eq!(
            snapshot.events.len(),
            1,
            "a rejected change writes no event"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn schema_rejects_broken_relationships() {
        let path = path();
        let (application_id, resource_id) = seed(&path);
        let db = open(&path).unwrap();
        let observation = |application: Option<&str>, resource: Option<&str>, until: i64| {
            db.execute(
                "INSERT INTO observations (id, application_id, resource_id, result, summary, source, observed_at, valid_until, recorded_at)
                 VALUES (?1, ?2, ?3, 'healthy', 'ok', 'manual', 10, ?4, 10)",
                params![Uuid::new_v4().to_string(), application, resource, until],
            )
        };
        let app = Some(application_id.as_str());
        let res = Some(resource_id.as_str());
        assert!(observation(app, res, 20).is_err(), "two subjects");
        assert!(observation(None, None, 20).is_err(), "no subject");
        assert!(
            observation(Some("ghost"), None, 20).is_err(),
            "missing application"
        );
        assert!(
            observation(None, Some("ghost"), 20).is_err(),
            "missing resource"
        );
        assert!(
            observation(app, None, 10).is_err(),
            "validity must end after the observation"
        );
        assert!(observation(app, None, 20).is_ok());
        assert!(
            db.execute(
                "INSERT INTO events (id, application_id, kind, outcome, title, source, occurred_at, recorded_at)
                 VALUES ('orphan', ?1, 'observation', 'success', 'Observed', 'manual', 1, 1)",
                [&application_id],
            )
            .is_err(),
            "an observation event must reference its observation"
        );
        assert!(
            db.execute(
                "INSERT INTO incidents (id, application_id, title, severity, status, source, opened_at, updated_at)
                 VALUES ('closed', ?1, 'Done', 'info', 'resolved', 'manual', 1, 1)",
                [&application_id],
            )
            .is_err(),
            "a resolved incident needs its resolution time"
        );
        assert!(
            db.execute(
                "UPDATE observations SET source = 'docker' WHERE application_id = ?1",
                [&application_id]
            )
            .is_err(),
            "only manual sources exist in this schema version"
        );
        assert!(
            db.execute("DELETE FROM applications WHERE id = ?1", [&application_id])
                .is_err(),
            "history keeps its subject"
        );
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    /// `count` automated results for a subject, 5 minutes apart from `first`, each with its
    /// event, as supervision writes them. Returns their observation IDs, oldest first.
    fn automated(path: &Path, subject: &Subject, first: i64, count: i64) -> Vec<String> {
        let mut db = open(path).unwrap();
        let tx = db.transaction().unwrap();
        let (application_id, resource_id) = subject.columns();
        let ids = (0..count)
            .map(|index| {
                let at = first + index * 5 * 60_000;
                let id = Uuid::new_v4().to_string();
                tx.execute(
                    "INSERT INTO observations (id, application_id, resource_id, result, summary, source, observed_at, valid_until, recorded_at,
                                               http_url, http_expected_status, http_status, http_latency_ms, http_error, http_redirects)
                     VALUES (?1, ?2, ?3, 'healthy', 'HTTP 200 as expected', 'http', ?4, ?5, ?4, 'https://x.example/', 200, 200, 5, NULL, '[]')",
                    params![id, application_id, resource_id, at, at + 15 * 60_000],
                )
                .unwrap();
                tx.execute(
                    "INSERT INTO events (id, application_id, resource_id, kind, outcome, title, detail, source, observation_id, occurred_at, recorded_at)
                     VALUES (?1, ?2, ?3, 'observation', 'success', 'HTTP check: Healthy', 'HTTP 200 as expected', 'http', ?4, ?5, ?5)",
                    params![Uuid::new_v4().to_string(), application_id, resource_id, id, at],
                )
                .unwrap();
                id
            })
            .collect();
        tx.commit().unwrap();
        ids
    }

    fn millis(time: &str) -> i64 {
        parse_time(time, "time").unwrap()
    }

    #[test]
    fn a_snapshot_carries_every_manual_record_but_only_the_latest_automated_results() {
        let path = path();
        let (application_id, resource_id) = seed(&path);
        let shop = Subject::Application { id: application_id };
        let host = Subject::Resource { id: resource_id };
        let limit = AUTOMATED_HISTORY_LIMIT;
        let minutes = |count: i64| count * 60_000;
        let first = now_ms() - minutes(30 * 24 * 60);
        let host_runs = automated(&path, &host, first, limit + 5);
        // Exactly the limit: nothing is left out.
        let shop_runs = automated(&path, &shop, first, limit);
        // Manual records older than every automated result, and one among those left out.
        let early = format_time(first - minutes(60)).unwrap();
        let old_observation = record_observation(
            &path,
            ObservationInput {
                observed_at: Some(early.clone()),
                ..observation(host.clone())
            },
        )
        .unwrap();
        let among_left_out = record_observation(
            &path,
            ObservationInput {
                observed_at: format_time(first + minutes(11)),
                ..observation(host.clone())
            },
        )
        .unwrap();
        let old_event = record_event(
            &path,
            EventInput {
                occurred_at: Some(early),
                ..event(host.clone())
            },
        )
        .unwrap();
        let opened = open_incident(&path, incident(host.clone())).unwrap();

        let snapshot = list(&path).unwrap();
        let automated_of = |subject: &Subject| -> Vec<String> {
            snapshot
                .observations
                .iter()
                .filter(|item| &item.subject == subject && item.source == HTTP)
                .map(|item| item.id.clone())
                .collect()
        };
        // The latest results, newest first; the five oldest stay in the database only.
        let kept: Vec<_> = host_runs[5..].iter().rev().cloned().collect();
        assert_eq!(automated_of(&host), kept);
        assert_eq!(automated_of(&shop).len(), shop_runs.len());
        assert!(snapshot.observations.contains(&old_observation));
        assert!(snapshot.observations.contains(&among_left_out));
        // Every loaded automated observation brings its event, and no other automated event.
        let mut loaded: Vec<_> = snapshot
            .observations
            .iter()
            .filter(|item| item.source == HTTP)
            .map(|item| Some(item.id.clone()))
            .collect();
        let mut with_events: Vec<_> = snapshot
            .events
            .iter()
            .filter(|item| item.source == HTTP)
            .map(|item| item.observation_id.clone())
            .collect();
        loaded.sort();
        with_events.sort();
        assert_eq!(loaded, with_events);
        assert!(snapshot.events.contains(&old_event));
        assert!(snapshot
            .events
            .iter()
            .any(|item| item.incident_id.as_deref() == Some(opened.id.as_str())));
        assert_eq!(snapshot.events.len(), snapshot.observations.len() + 2);
        // Newest first across both sources.
        assert!(snapshot
            .observations
            .windows(2)
            .all(|pair| millis(&pair[0].observed_at) >= millis(&pair[1].observed_at)));
        assert!(snapshot
            .events
            .windows(2)
            .all(|pair| millis(&pair[0].occurred_at) >= millis(&pair[1].occurred_at)));
        // What was left out is said, so that the window never implies there is nothing older.
        assert_eq!(
            snapshot.automated_history,
            AutomatedHistory {
                limit,
                truncated: vec![TruncatedHistory {
                    subject: host.clone(),
                    since: format_time(first + minutes(5 * 5)).unwrap(),
                }],
            }
        );

        // A new result moves the window forward by one.
        automated(&path, &host, first + minutes(5 * (limit + 5)), 1);
        let moved = list(&path).unwrap();
        assert_eq!(
            moved.automated_history.truncated[0].since,
            format_time(first + minutes(5 * 6)).unwrap()
        );
        assert_eq!(automated_of(&host).len(), usize::try_from(limit).unwrap());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_tie_at_the_cut_never_loads_an_event_without_its_observation() {
        let path = path();
        let (_, resource_id) = seed(&path);
        let host = Subject::Resource { id: resource_id };
        let first = now_ms() - 24 * 60 * 60_000;
        // Two results at the same instant: rowid orders them, and only the later one is kept.
        let cut = automated(&path, &host, first, 1);
        automated(&path, &host, first, AUTOMATED_HISTORY_LIMIT);
        let snapshot = list(&path).unwrap();
        let mut observed: Vec<_> = snapshot
            .observations
            .iter()
            .map(|item| Some(item.id.clone()))
            .collect();
        let mut evented: Vec<_> = snapshot
            .events
            .iter()
            .map(|item| item.observation_id.clone())
            .collect();
        observed.sort();
        evented.sort();
        assert_eq!(
            observed.len(),
            usize::try_from(AUTOMATED_HISTORY_LIMIT).unwrap()
        );
        assert_eq!(observed, evented);
        assert!(!observed.contains(&Some(cut[0].clone())));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn older_automated_results_load_a_page_at_a_time_without_gaps() {
        let path = path();
        let (_, resource_id) = seed(&path);
        let host = Subject::Resource { id: resource_id };
        let limit = usize::try_from(AUTOMATED_HISTORY_LIMIT).unwrap();
        let first = now_ms() - 10 * 24 * 60 * 60_000;
        // Oldest first: a full page and three more beyond the snapshot's window.
        let runs = automated(&path, &host, first, AUTOMATED_HISTORY_LIMIT * 2 + 3);
        record_observation(
            &path,
            ObservationInput {
                observed_at: format_time(first + 1),
                ..observation(host.clone())
            },
        )
        .unwrap();
        let snapshot = list(&path).unwrap();
        let oldest_shown = snapshot
            .observations
            .iter()
            .rfind(|item| item.source == HTTP)
            .unwrap();
        assert_eq!(oldest_shown.id, runs[limit + 3]);

        let page = older_observations(&path, &host, &oldest_shown.id).unwrap();
        assert!(page.more);
        let ids: Vec<_> = page
            .observations
            .iter()
            .map(|item| item.id.clone())
            .collect();
        assert_eq!(
            ids,
            runs[3..limit + 3].iter().rev().cloned().collect::<Vec<_>>()
        );
        assert!(page.observations.iter().all(|item| item.source == HTTP));
        // A new result meanwhile changes nothing for the next page: it is anchored, not counted.
        automated(&path, &host, now_ms() - 60_000, 1);
        let last = older_observations(&path, &host, ids.last().unwrap()).unwrap();
        assert!(!last.more);
        assert_eq!(
            last.observations
                .iter()
                .map(|item| item.id.clone())
                .collect::<Vec<_>>(),
            runs[..3].iter().rev().cloned().collect::<Vec<_>>()
        );
        let none = older_observations(&path, &host, &runs[0]).unwrap();
        assert!(none.observations.is_empty() && !none.more);

        // The anchor must be one of the subject's own observations.
        let (other_application, _) = seed(&path);
        let other = Subject::Application {
            id: other_application,
        };
        for (subject, before) in [(&other, runs[5].as_str()), (&host, "ghost")] {
            let error = older_observations(&path, subject, before).unwrap_err();
            assert_eq!(error.code, "invalid_input", "{}", error.message);
        }
        std::fs::remove_file(path).unwrap();
    }

    /// Every automated event of these subjects with its time, in timeline order: what pages
    /// must add up to.
    fn automated_timeline(path: &Path, subjects: &[&Subject]) -> Vec<(String, i64)> {
        let db = open(path).unwrap();
        let mut statement = db
            .prepare(
                "SELECT id, application_id, resource_id, occurred_at FROM events WHERE source = 'http'
                 ORDER BY occurred_at DESC, recorded_at DESC, rowid DESC",
            )
            .unwrap();
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    Subject::from_columns(row.get(1)?, row.get(2)?)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows.into_iter()
            .filter(|(_, subject, _)| subjects.contains(&subject))
            .map(|(id, _, at)| (id, at))
            .collect()
    }

    #[test]
    fn older_automated_events_page_through_several_systems_without_gaps() {
        let path = path();
        let (application_id, resource_id) = seed(&path);
        let shop = Subject::Application { id: application_id };
        let host = Subject::Resource { id: resource_id };
        let (other_application, _) = seed(&path);
        let other = Subject::Application {
            id: other_application,
        };
        let first = now_ms() - 10 * 24 * 60 * 60_000;
        // Both systems ran at the same instants, so only the rowid orders each pair; one of them
        // also ran in between.
        automated(&path, &shop, first, 150);
        automated(&path, &host, first, 150);
        automated(&path, &host, first + 60_000, 100);
        // Neither another system's results nor manual records belong in these pages.
        automated(&path, &other, first, 50);
        record_event(
            &path,
            EventInput {
                occurred_at: format_time(first + 7 * 60_000),
                ..event(host.clone())
            },
        )
        .unwrap();
        let timeline = automated_timeline(&path, &[&shop, &host]);
        let ids: Vec<_> = timeline.iter().map(|(id, _)| id.clone()).collect();
        assert_eq!(ids.len(), 400);
        let limit = usize::try_from(AUTOMATED_HISTORY_LIMIT).unwrap();
        // Anchored on one of two results at the same instant: the other, ordered after it, is
        // the first of the page.
        let anchor = 36;
        assert_eq!(timeline[anchor].1, timeline[anchor + 1].1);
        let both = [shop.clone(), host.clone()];
        let page = older_events(&path, &both, &ids[anchor]).unwrap();
        assert!(page.more);
        let shown: Vec<_> = page.events.iter().map(|item| item.id.clone()).collect();
        assert_eq!(shown, ids[anchor + 1..anchor + 1 + limit]);
        assert!(page.events.iter().all(|item| item.source == HTTP));

        // A new result meanwhile changes nothing for the next page: it is anchored, not counted.
        // Repeating a subject changes nothing either.
        automated(&path, &host, now_ms() - 60_000, 1);
        let rest = older_events(
            &path,
            &[host.clone(), shop.clone(), host.clone()],
            shown.last().unwrap(),
        )
        .unwrap();
        assert!(!rest.more);
        assert_eq!(
            rest.events
                .iter()
                .map(|item| item.id.clone())
                .collect::<Vec<_>>(),
            ids[anchor + 1 + limit..]
        );
        let none = older_events(&path, &both, ids.last().unwrap()).unwrap();
        assert!(none.events.is_empty() && !none.more);
        // Exactly a page left is the last page; one more is not.
        let exact = older_events(&path, &both, &ids[ids.len() - limit - 1]).unwrap();
        assert_eq!((exact.events.len(), exact.more), (limit, false));
        let over = older_events(&path, &both, &ids[ids.len() - limit - 2]).unwrap();
        assert_eq!((over.events.len(), over.more), (limit, true));

        // The anchor must be one of these systems' own events, and every system must exist.
        let other_event = &automated_timeline(&path, &[&other])[0].0;
        let shop_event = &automated_timeline(&path, &[&shop])[0].0;
        for (subjects, before) in [
            (vec![], shop_event.as_str()),
            (vec![shop.clone()], other_event.as_str()),
            (vec![shop.clone()], "ghost"),
            (
                vec![shop.clone(), Subject::Resource { id: "ghost".into() }],
                shop_event.as_str(),
            ),
        ] {
            let error = older_events(&path, &subjects, before).unwrap_err();
            assert_eq!(error.code, "invalid_input", "{}", error.message);
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn snapshot_reads_follow_indexes_instead_of_scanning_history() {
        let path = path();
        let db = open(&path).unwrap();
        let plan = |sql: &str, params: &[&dyn rusqlite::ToSql]| -> Vec<String> {
            db.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap()
                .query_map(params, |row| row.get(3))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        let observations = format!("{OBSERVATION_SELECT} {WINDOWED_OBSERVATIONS}");
        let events = format!("{EVENT_SELECT} {WINDOWED_EVENTS}");
        let older = OLDER_AUTOMATED.map(|filter| format!("{OBSERVATION_SELECT} {filter}"));
        let page_events = format!("{EVENT_SELECT} {EVENTS_BY_ROWID}");
        let cases: [(&str, &[&dyn rusqlite::ToSql], &str); 10] = [
            (
                LATEST_AUTOMATED[0],
                &[&"app", &1],
                "observations_application_id",
            ),
            (
                LATEST_AUTOMATED[1],
                &[&"host", &1],
                "observations_resource_id",
            ),
            (&events, &[&"[]"], "events_observation_id"),
            (&observations, &[&"[]"], "observations_manual"),
            (&events, &[&"[]"], "events_manual"),
            (&older[0], &[&"app", &9, &9, &9, &1], "observed_at<"),
            (&older[1], &[&"host", &9, &9, &9, &1], "observed_at<"),
            (
                OLDER_AUTOMATED_EVENTS[0],
                &[&"app", &9, &9, &9, &1],
                "events_application_id (application_id=? AND occurred_at<?)",
            ),
            (
                OLDER_AUTOMATED_EVENTS[1],
                &[&"host", &9, &9, &9, &1],
                "events_resource_id (resource_id=? AND occurred_at<?)",
            ),
            (&page_events, &[&"[]"], "INTEGER PRIMARY KEY"),
        ];
        for (sql, params, index) in cases {
            let steps = plan(sql, params);
            assert!(
                steps.iter().any(|step| step.contains(index)),
                "{sql}\n{steps:#?}"
            );
            // Only the manual records are ever read in full.
            assert!(
                steps.iter().all(|step| {
                    !(step.starts_with("SCAN observations") || step.starts_with("SCAN events"))
                        || step.contains("_manual")
                }),
                "{sql}\n{steps:#?}"
            );
        }
        drop(db);
        std::fs::remove_file(path).unwrap();
    }
}
