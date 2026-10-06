//! Periodic supervision of HTTP checks, only while DevHQ is open. One loop on Tauri's async
//! runtime decides when each supervised check is due. Every run, scheduled or asked for, goes
//! through here, so a check never has two runs at once: asking for a run while one is in flight
//! waits for that run's observation instead of recording a second one.
//!
//! Policy:
//! - The next run is due one interval after the previous run ended, whatever started it, so a
//!   manual run also restarts the wait. Missed runs are never made up: after a pause, a closed
//!   window or a slow run, an overdue check runs once.
//! - On launch, overdue checks start one after another instead of all at once.
//! - No retries: after any failure, of the checked system or of DevHQ, the check waits a full
//!   interval. A DevHQ failure records nothing, stays visible and never stops other checks.
//! - Pausing lets the run in flight finish (its timeout bounds it) and records it; removing a
//!   check cancels its run in flight and records nothing. Neither schedules another run.
//! - Shutting down cancels every run in flight and ends the loop. Nothing is recorded once it
//!   has returned: a run whose request already ended cannot be aborted while it writes, so its
//!   write checks the supervision's lifeline in its transaction (see `records::Lifeline`).

use crate::http_check::{self, HttpCheck};
use crate::inventory::ApiError;
use crate::records::{self, Lifeline, Observation, Subject};
use serde::Serialize;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;
use tokio::sync::{watch, Notify};
use tokio::task::AbortHandle;

const MINUTE_MS: i64 = 60_000;
/// The loop plans again at least this often, so a changed system clock or a machine waking up
/// is noticed without trusting one long timer.
const MAX_SLEEP_MS: i64 = 30_000;
/// On launch, overdue checks start after this delay and this far apart.
const LAUNCH_DELAY_MS: i64 = 10_000;
const LAUNCH_SPACING_MS: i64 = 10_000;

/// Milliseconds since the Unix epoch. Tests pass a simulated clock.
pub(crate) type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;
pub(crate) type RunFuture = Pin<Box<dyn Future<Output = Result<Observation, ApiError>> + Send>>;
/// Runs a subject's check once and records its observation while the lifeline is alive. Tests
/// pass a scripted one.
pub(crate) type Runner = Arc<dyn Fn(Subject, Lifeline) -> RunFuture + Send + Sync>;
type RunResult = Option<Result<Observation, ApiError>>;

/// What the window shows about supervision. Configuration (interval, active or paused) comes
/// with the inventory; this is what only the running process knows.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MonitorStatus {
    pub checks: Vec<CheckStatus>,
    /// DevHQ could not read the checks' configuration.
    pub error: Option<String>,
    /// Observations recorded by runs since launch; the window reloads when it grows.
    pub recorded: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CheckStatus {
    pub subject: Subject,
    pub running: bool,
    pub last_run_at: Option<String>,
    /// Only while supervision is active and no run is in flight.
    pub next_run_at: Option<String>,
    /// The last run failed inside DevHQ and recorded nothing. Cleared by the next good run.
    pub last_error: Option<RunError>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunError {
    pub message: String,
    pub at: String,
}

#[derive(Clone)]
pub struct Monitor {
    inner: Arc<Inner>,
}

struct Inner {
    path: PathBuf,
    clock: Clock,
    runner: Runner,
    /// Ended by `shutdown`, after which no run commits anything.
    lifeline: Lifeline,
    state: Mutex<State>,
    /// Reading the configuration and applying it happen as one step.
    refresh: Mutex<()>,
    wake: Notify,
    status: watch::Sender<MonitorStatus>,
}

#[derive(Default)]
struct State {
    stopped: bool,
    loaded: bool,
    /// In configuration order.
    entries: Vec<Entry>,
    error: Option<String>,
    recorded: u64,
    next_run_id: u64,
    watcher: Option<AbortHandle>,
}

struct Entry {
    subject: Subject,
    /// False for a run asked for a subject without a check; it fails on its own and is dropped.
    configured: bool,
    /// Interval in minutes while supervision is active.
    schedule: Option<i64>,
    last_run_at: Option<i64>,
    next_run_at: Option<i64>,
    run: Option<Run>,
    last_error: Option<RunError>,
}

struct Run {
    id: u64,
    result: watch::Receiver<RunResult>,
    abort: AbortHandle,
}

impl Entry {
    fn new(subject: Subject, last_run_at: Option<i64>) -> Self {
        Self {
            subject,
            configured: false,
            schedule: None,
            last_run_at,
            next_run_at: None,
            run: None,
            last_error: None,
        }
    }
}

/// One interval after the last run; never in the past, because missed runs are not made up,
/// and never more than one interval away, in case the clock moved backwards.
fn next_due(last_run_at: Option<i64>, interval_minutes: i64, now: i64) -> i64 {
    let interval = interval_minutes * MINUTE_MS;
    last_run_at.map_or(now, |last| (last + interval).clamp(now, now + interval))
}

/// On launch, checks that are already due start one after another: never-run checks first,
/// then the ones that have waited longest.
fn stagger(entries: &mut [Entry], now: i64) {
    let mut due: Vec<&mut Entry> = entries
        .iter_mut()
        .filter(|entry| entry.next_run_at.is_some_and(|at| at <= now))
        .collect();
    due.sort_by_key(|entry| entry.last_run_at);
    for (position, entry) in (0_i64..).zip(due) {
        entry.next_run_at = Some(now + LAUNCH_DELAY_MS + LAUNCH_SPACING_MS * position);
    }
}

/// Applies the stored configuration. A check whose supervision changed is planned again from
/// its last run; a removed check's run in flight is cancelled.
fn apply(state: &mut State, checks: Vec<HttpCheck>, last_runs: &HashMap<Subject, i64>, now: i64) {
    let mut previous = std::mem::take(&mut state.entries);
    let mut entries = Vec::with_capacity(checks.len());
    for check in checks {
        let schedule = match check.monitor.as_str() {
            "active" => check.monitor_interval_minutes,
            _ => None,
        };
        let mut entry = match previous
            .iter()
            .position(|item| item.subject == check.subject)
        {
            Some(index) => previous.swap_remove(index),
            None => Entry::new(
                check.subject.clone(),
                last_runs.get(&check.subject).copied(),
            ),
        };
        let replan = !entry.configured || entry.schedule != schedule;
        entry.configured = true;
        entry.schedule = schedule;
        if replan && entry.run.is_none() {
            entry.next_run_at = schedule.map(|minutes| next_due(entry.last_run_at, minutes, now));
        }
        entries.push(entry);
    }
    for entry in previous {
        match (&entry.run, entry.configured) {
            (Some(run), true) => run.abort.abort(),
            (Some(_), false) => entries.push(entry),
            (None, _) => {}
        }
    }
    if !state.loaded {
        stagger(&mut entries, now);
        state.loaded = true;
    }
    state.entries = entries;
}

fn status_of(state: &State) -> MonitorStatus {
    MonitorStatus {
        checks: state
            .entries
            .iter()
            .filter(|entry| entry.configured)
            .map(|entry| CheckStatus {
                subject: entry.subject.clone(),
                running: entry.run.is_some(),
                last_run_at: entry.last_run_at.and_then(records::format_time),
                next_run_at: entry.next_run_at.and_then(records::format_time),
                last_error: entry.last_error.clone(),
            })
            .collect(),
        error: state.error.clone(),
        recorded: state.recorded,
    }
}

fn cancelled() -> ApiError {
    ApiError::cancelled(
        "The HTTP check was cancelled before it finished: it was removed or DevHQ is closing. Nothing was recorded.",
    )
}

impl Monitor {
    /// Supervision of the checks stored at `path`, running them for real.
    pub fn new(path: PathBuf) -> Self {
        let database = path.clone();
        Self::with(
            path,
            Arc::new(records::now_ms),
            Arc::new(move |subject: Subject, lifeline: Lifeline| -> RunFuture {
                let path = database.clone();
                Box::pin(async move { http_check::run_http_check(&path, subject, &lifeline).await })
            }),
        )
    }

    pub(crate) fn with(path: PathBuf, clock: Clock, runner: Runner) -> Self {
        Self {
            inner: Arc::new(Inner {
                path,
                clock,
                runner,
                lifeline: Lifeline::default(),
                state: Mutex::default(),
                refresh: Mutex::default(),
                wake: Notify::new(),
                status: watch::Sender::new(MonitorStatus::default()),
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn publish(&self, state: &State) {
        let status = status_of(state);
        self.inner.status.send_if_modified(|current| {
            let changed = *current != status;
            *current = status;
            changed
        });
    }

    #[cfg(test)]
    pub(crate) fn status(&self) -> MonitorStatus {
        self.inner.status.borrow().clone()
    }

    /// The loop. It ends after `shutdown`.
    pub async fn run(self) {
        loop {
            let loaded = self.lock().loaded;
            if !loaded {
                // Until the configuration can be read, nothing runs; reading is tried again
                // on the next pass, 30 s later. It must not wake the loop itself, or a
                // database that cannot be read would be read again in a busy loop.
                self.read_configuration();
            }
            let Some(wait) = self.start_due_runs() else {
                return;
            };
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                () = self.inner.wake.notified() => {}
            }
        }
    }

    /// Reads the configuration again and applies it at once. Called after every change to a
    /// check, so pausing, resuming and removing take effect immediately.
    pub fn refresh(&self) {
        self.read_configuration();
        self.inner.wake.notify_one();
    }

    fn read_configuration(&self) {
        let _serial = self
            .inner
            .refresh
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let read = http_check::supervision(&self.inner.path);
        let now = (self.inner.clock)();
        let mut state = self.lock();
        if state.stopped {
            return;
        }
        match read {
            Ok((checks, last_runs)) => {
                state.error = None;
                apply(&mut state, checks, &last_runs, now);
            }
            Err(error) => state.error = Some(error.message),
        }
        self.publish(&state);
    }

    /// Starts every due check and says how long to wait before planning again; None once
    /// stopped.
    fn start_due_runs(&self) -> Option<Duration> {
        let now = (self.inner.clock)();
        let mut state = self.lock();
        if state.stopped {
            return None;
        }
        for index in 0..state.entries.len() {
            let entry = &state.entries[index];
            if entry.schedule.is_some()
                && entry.run.is_none()
                && entry.next_run_at.is_some_and(|at| at <= now)
            {
                self.start(&mut state, index);
            }
        }
        let next = state
            .entries
            .iter()
            .filter_map(|entry| entry.next_run_at)
            .min();
        self.publish(&state);
        let wait = next.map_or(MAX_SLEEP_MS, |at| (at - now).clamp(0, MAX_SLEEP_MS));
        Some(Duration::from_millis(u64::try_from(wait).unwrap_or(0)))
    }

    fn start(&self, state: &mut State, index: usize) -> watch::Receiver<RunResult> {
        state.next_run_id += 1;
        let id = state.next_run_id;
        let entry = &mut state.entries[index];
        let (sender, receiver) = watch::channel(None);
        let run = (self.inner.runner)(entry.subject.clone(), self.inner.lifeline.clone());
        let monitor = self.clone();
        let task = tokio::spawn(async move {
            let result = run.await;
            monitor.finish(id, &result);
            sender.send_replace(Some(result));
        });
        entry.run = Some(Run {
            id,
            result: receiver.clone(),
            abort: task.abort_handle(),
        });
        entry.next_run_at = None;
        receiver
    }

    fn finish(&self, id: u64, result: &Result<Observation, ApiError>) {
        let now = (self.inner.clock)();
        let mut state = self.lock();
        let Some(index) = state
            .entries
            .iter()
            .position(|entry| entry.run.as_ref().is_some_and(|run| run.id == id))
        else {
            // Cancelled meanwhile; its entry is gone.
            return;
        };
        if result.is_ok() {
            state.recorded += 1;
        }
        let entry = &mut state.entries[index];
        entry.run = None;
        if entry.configured {
            entry.last_run_at = Some(now);
            entry.next_run_at = entry.schedule.map(|minutes| now + minutes * MINUTE_MS);
            entry.last_error = result.as_ref().err().map(|error| RunError {
                message: error.message.clone(),
                at: records::format_time(now).unwrap_or_default(),
            });
        } else {
            state.entries.remove(index);
        }
        self.publish(&state);
        drop(state);
        self.inner.wake.notify_one();
    }

    /// Runs the subject's check now, or joins the run already in flight, and returns the
    /// observation it recorded.
    pub async fn run_now(&self, subject: Subject) -> Result<Observation, ApiError> {
        let receiver = {
            let mut state = self.lock();
            if state.stopped {
                return Err(cancelled());
            }
            let index = match state
                .entries
                .iter()
                .position(|entry| entry.subject == subject)
            {
                Some(index) => index,
                None => {
                    state.entries.push(Entry::new(subject, None));
                    state.entries.len() - 1
                }
            };
            let receiver = match &state.entries[index].run {
                Some(run) => run.result.clone(),
                None => self.start(&mut state, index),
            };
            self.publish(&state);
            receiver
        };
        outcome(receiver).await
    }

    /// Sends the status now and after every change until `send` fails. A new watcher (the
    /// window reloaded) replaces the previous one.
    pub fn watch(&self, send: impl Fn(MonitorStatus) -> bool + Send + 'static) {
        let mut receiver = self.inner.status.subscribe();
        let task = tokio::spawn(async move {
            loop {
                let status = receiver.borrow_and_update().clone();
                if !send(status) || receiver.changed().await.is_err() {
                    return;
                }
            }
        });
        let mut state = self.lock();
        if let Some(previous) = state.watcher.replace(task.abort_handle()) {
            previous.abort();
        }
        if state.stopped {
            task.abort();
        }
    }

    /// Stops supervision for good: cancels every run in flight and ends the loop. Nothing a
    /// cancelled run was doing is recorded, and nothing at all once this returns. A run already
    /// committing its result is waited for (it holds the lifeline only from SQLite's write lock
    /// to its connection closing); any later write is refused in its transaction.
    pub fn shutdown(&self) {
        // Before the state lock, which a finishing run takes: never hold it while waiting.
        self.inner.lifeline.end();
        let mut state = self.lock();
        state.stopped = true;
        for entry in &mut state.entries {
            if let Some(run) = entry.run.take() {
                run.abort.abort();
            }
        }
        if let Some(watcher) = state.watcher.take() {
            watcher.abort();
        }
        drop(state);
        self.inner.wake.notify_one();
    }
}

async fn outcome(mut receiver: watch::Receiver<RunResult>) -> Result<Observation, ApiError> {
    let result = match receiver.wait_for(Option::is_some).await {
        Ok(result) => result.clone(),
        // The run was cancelled: its sender went away with it.
        Err(_) => None,
    };
    result.unwrap_or_else(|| Err(cancelled()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http_check::tests::{resource, serve};
    use crate::http_check::{
        remove_http_check, save_http_check, set_http_check_paused, HttpCheckInput,
    };
    use crate::inventory::{list, open};
    use crate::records::{HttpEvidence, NewObservation};
    use std::path::Path;
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
    use tokio::time::{sleep, Instant};
    use uuid::Uuid;

    const SECOND: Duration = Duration::from_secs(1);
    const MINUTE: Duration = Duration::from_secs(60);

    fn path() -> PathBuf {
        std::env::temp_dir().join(format!("devhq-monitor-test-{}.sqlite3", Uuid::new_v4()))
    }

    fn supervise(path: &Path, subject: &Subject, url: &str, interval: Option<i64>, valid: i64) {
        save_http_check(
            path,
            HttpCheckInput {
                subject: subject.clone(),
                url: url.into(),
                expected_status: 200,
                timeout_seconds: 5,
                valid_for_minutes: valid,
                monitor_interval_minutes: interval,
            },
        )
        .unwrap();
    }

    /// Simulated wall clock: follows Tokio's paused time, which only moves when every task waits.
    fn simulated_clock() -> Clock {
        let base = records::now_ms();
        let start = Instant::now();
        Arc::new(move || base + i64::try_from(start.elapsed().as_millis()).unwrap())
    }

    enum Behaviour {
        /// Succeeds after this much (simulated) time.
        Takes(Duration),
        /// DevHQ fails: nothing is recorded.
        Fails,
        /// Waits until the test releases it.
        Gate(Arc<Notify>),
    }

    /// A scripted runner that records when each run started and whether any overlapped.
    #[derive(Default)]
    struct Script {
        starts: Mutex<Vec<(Subject, i64)>>,
        running: Mutex<HashMap<Subject, usize>>,
        overlaps: AtomicUsize,
        cancelled: AtomicUsize,
        behaviour: Mutex<HashMap<Subject, Behaviour>>,
    }

    impl Script {
        fn set(&self, subject: &Subject, behaviour: Behaviour) {
            self.behaviour
                .lock()
                .unwrap()
                .insert(subject.clone(), behaviour);
        }
        /// When each run of `subject` started, in milliseconds after `origin`.
        fn starts(&self, subject: &Subject, origin: i64) -> Vec<i64> {
            self.starts
                .lock()
                .unwrap()
                .iter()
                .filter(|(item, _)| item == subject)
                .map(|(_, at)| at - origin)
                .collect()
        }
    }

    /// Counts a run as cancelled if it is dropped before it finished.
    struct InFlight {
        script: Arc<Script>,
        subject: Subject,
        finished: bool,
    }

    impl Drop for InFlight {
        fn drop(&mut self) {
            *self
                .script
                .running
                .lock()
                .unwrap()
                .get_mut(&self.subject)
                .unwrap() -= 1;
            if !self.finished {
                self.script.cancelled.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    fn observation(subject: &Subject, now: i64) -> Observation {
        Observation {
            id: Uuid::new_v4().to_string(),
            subject: subject.clone(),
            result: "healthy".into(),
            summary: "scripted".into(),
            source: "http".into(),
            observed_at: records::format_time(now).unwrap(),
            valid_until: records::format_time(now + 60 * MINUTE_MS).unwrap(),
            recorded_at: records::format_time(now).unwrap(),
            http: None,
            agent: None,
        }
    }

    fn scripted(script: &Arc<Script>, clock: &Clock) -> Runner {
        let script = Arc::clone(script);
        let clock = Arc::clone(clock);
        Arc::new(move |subject: Subject, _: Lifeline| -> RunFuture {
            let script = Arc::clone(&script);
            let clock = Arc::clone(&clock);
            Box::pin(async move {
                script
                    .starts
                    .lock()
                    .unwrap()
                    .push((subject.clone(), clock()));
                {
                    let mut running = script.running.lock().unwrap();
                    let count = running.entry(subject.clone()).or_default();
                    if *count > 0 {
                        script.overlaps.fetch_add(1, Ordering::SeqCst);
                    }
                    *count += 1;
                }
                let mut guard = InFlight {
                    script: Arc::clone(&script),
                    subject: subject.clone(),
                    finished: false,
                };
                enum Step {
                    Wait(Duration),
                    Fail,
                    Gate(Arc<Notify>),
                }
                let step = match script.behaviour.lock().unwrap().get(&subject) {
                    Some(Behaviour::Takes(time)) => Step::Wait(*time),
                    Some(Behaviour::Fails) => Step::Fail,
                    Some(Behaviour::Gate(gate)) => Step::Gate(Arc::clone(gate)),
                    None => Step::Wait(Duration::ZERO),
                };
                let result = match step {
                    Step::Wait(time) => {
                        sleep(time).await;
                        Ok(observation(&subject, clock()))
                    }
                    Step::Fail => Err(ApiError::database("disk I/O error")),
                    Step::Gate(gate) => {
                        gate.notified().await;
                        Ok(observation(&subject, clock()))
                    }
                };
                guard.finished = true;
                result
            })
        })
    }

    struct Harness {
        path: PathBuf,
        clock: Clock,
        origin: i64,
        script: Arc<Script>,
        monitor: Monitor,
        task: tokio::task::JoinHandle<()>,
    }

    impl Harness {
        /// Starts supervision of what is stored at `path`, as a launch does.
        fn launch(path: PathBuf) -> Self {
            let clock = simulated_clock();
            let script = Arc::new(Script::default());
            let monitor =
                Monitor::with(path.clone(), Arc::clone(&clock), scripted(&script, &clock));
            let origin = clock();
            let task = tokio::spawn(monitor.clone().run());
            Self {
                path,
                clock,
                origin,
                script,
                monitor,
                task,
            }
        }
        fn check(&self, subject: &Subject) -> CheckStatus {
            self.monitor
                .status()
                .checks
                .into_iter()
                .find(|item| &item.subject == subject)
                .unwrap()
        }
        fn at(&self, time: &Option<String>) -> Option<i64> {
            time.as_deref().map(|value| {
                i64::try_from(
                    time::OffsetDateTime::parse(
                        value,
                        &time::format_description::well_known::Rfc3339,
                    )
                    .unwrap()
                    .unix_timestamp_nanos()
                        / 1_000_000,
                )
                .unwrap()
                    - self.origin
            })
        }
        fn pause(&self, subject: &Subject, paused: bool) {
            set_http_check_paused(&self.path, subject, paused).unwrap();
            self.monitor.refresh();
        }
    }

    const S: i64 = 1_000;
    const M: i64 = 60 * S;

    #[test]
    fn plans_from_the_last_run_without_making_up_missed_runs() {
        let now = 1_000_000 * M;
        assert_eq!(next_due(None, 5, now), now, "never run: due now");
        assert_eq!(next_due(Some(now - 2 * M), 5, now), now + 3 * M);
        assert_eq!(
            next_due(Some(now - 300 * M), 5, now),
            now,
            "60 missed runs become one run now"
        );
        assert_eq!(
            next_due(Some(now + 90 * M), 5, now),
            now + 5 * M,
            "a clock moved backwards waits one interval at most"
        );
        let mut entries: Vec<Entry> = [
            None,
            Some(now - 90 * M),
            Some(now - 2 * M),
            Some(now - 300 * M),
            None,
        ]
        .into_iter()
        .enumerate()
        .map(|(id, last)| {
            let mut entry = Entry::new(Subject::Resource { id: id.to_string() }, last);
            entry.next_run_at = Some(next_due(last, 5, now));
            entry
        })
        .collect();
        stagger(&mut entries, now);
        let planned: Vec<_> = entries
            .iter()
            .map(|entry| entry.next_run_at.unwrap() - now)
            .collect();
        // Never-run checks first (in order), then the longest waiting; the one not due keeps its time.
        assert_eq!(planned, [10 * S, 40 * S, 3 * M, 30 * S, 20 * S]);
    }

    #[tokio::test(start_paused = true)]
    async fn runs_each_supervised_check_every_interval_on_a_simulated_clock() {
        let path = path();
        let (every5, every10, on_demand, paused) = (
            resource(&path),
            resource(&path),
            resource(&path),
            resource(&path),
        );
        supervise(&path, &every5, "http://a.invalid/", Some(5), 15);
        supervise(&path, &every10, "http://b.invalid/", Some(10), 60);
        supervise(&path, &on_demand, "http://c.invalid/", None, 60);
        supervise(&path, &paused, "http://d.invalid/", Some(5), 15);
        set_http_check_paused(&path, &paused, true).unwrap();
        let harness = Harness::launch(path.clone());
        harness.script.set(&every5, Behaviour::Takes(2 * SECOND));
        harness.script.set(&every10, Behaviour::Takes(2 * SECOND));

        sleep(61 * MINUTE).await;
        let fives = harness.script.starts(&every5, harness.origin);
        // Launch spreads the overdue checks; then each runs one interval after the last ended.
        assert_eq!(fives[0], 10 * S);
        assert!(
            fives
                .windows(2)
                .all(|pair| pair[1] - pair[0] == 5 * M + 2 * S),
            "{fives:?}"
        );
        assert_eq!(fives.len(), 13);
        let tens = harness.script.starts(&every10, harness.origin);
        assert_eq!(tens[0], 20 * S);
        assert!(
            tens.windows(2)
                .all(|pair| pair[1] - pair[0] == 10 * M + 2 * S),
            "{tens:?}"
        );
        assert_eq!(tens.len(), 7);
        assert!(harness.script.starts(&on_demand, harness.origin).is_empty());
        assert!(harness.script.starts(&paused, harness.origin).is_empty());
        assert_eq!(harness.script.overlaps.load(Ordering::SeqCst), 0);

        let status = harness.check(&every5);
        let last = harness.at(&status.last_run_at).unwrap();
        assert_eq!(last, starts_last(&harness, &every5) + 2 * S);
        assert_eq!(harness.at(&status.next_run_at), Some(last + 5 * M));
        assert!(!status.running);
        assert_eq!(status.last_error, None);
        // Not supervised or paused: known, but nothing is planned.
        assert_eq!(harness.check(&on_demand).next_run_at, None);
        assert_eq!(harness.check(&paused).next_run_at, None);
        assert_eq!(harness.monitor.status().recorded, 13 + 7);
        harness.monitor.shutdown();
        std::fs::remove_file(path).unwrap();
    }

    fn starts_last(harness: &Harness, subject: &Subject) -> i64 {
        *harness
            .script
            .starts(subject, harness.origin)
            .last()
            .unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn a_relaunch_keeps_the_rhythm_and_runs_overdue_checks_once() {
        let path = path();
        let (recent, overdue, never) = (resource(&path), resource(&path), resource(&path));
        for subject in [&recent, &overdue, &never] {
            supervise(&path, subject, "http://x.invalid/", Some(5), 15);
        }
        // Evidence from before the app was closed.
        let now = records::now_ms();
        for (subject, ago) in [(&recent, 2 * M), (&overdue, 3 * 60 * M)] {
            records::store_observation(
                &path,
                NewObservation {
                    subject,
                    result: "healthy",
                    summary: "HTTP 200 as expected",
                    observed_at: now - ago,
                    valid_for_minutes: 15,
                    recorded_at: now - ago,
                    http: Some(&HttpEvidence {
                        url: "http://x.invalid/".into(),
                        expected_status: 200,
                        status: Some(200),
                        latency_ms: 3,
                        error: None,
                        redirects: vec![],
                    }),
                    reporter: None,
                    lifeline: None,
                },
            )
            .unwrap();
        }
        let harness = Harness::launch(path.clone());
        let before_launch = now - harness.origin;
        sleep(SECOND).await;
        assert_eq!(
            harness.at(&harness.check(&overdue).last_run_at),
            Some(before_launch - 3 * 60 * M)
        );
        sleep(4 * MINUTE).await;
        // Three hours overdue means one run, not 36; the recent one waits for its turn.
        assert_eq!(harness.script.starts(&never, harness.origin), [10 * S]);
        assert_eq!(harness.script.starts(&overdue, harness.origin), [20 * S]);
        assert_eq!(
            harness.script.starts(&recent, harness.origin),
            [before_launch - 2 * M + 5 * M]
        );
        harness.monitor.shutdown();
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn never_overlaps_and_a_manual_run_joins_the_scheduled_one() {
        let path = path();
        let slow = resource(&path);
        supervise(&path, &slow, "http://x.invalid/", Some(5), 15);
        let harness = Harness::launch(path.clone());
        // Longer than the interval: a real check is bounded by its 10 s timeout, this one is not.
        harness.script.set(&slow, Behaviour::Takes(7 * MINUTE));
        sleep(11 * SECOND).await;
        assert!(harness.check(&slow).running);
        // Asked for during the scheduled run: the same run, the same observation.
        let (first, second) = tokio::join!(
            harness.monitor.run_now(slow.clone()),
            harness.monitor.run_now(slow.clone())
        );
        let (first, second) = (first.unwrap(), second.unwrap());
        assert_eq!(first, second);
        assert_eq!(harness.script.starts(&slow, harness.origin), [10 * S]);
        assert_eq!(harness.monitor.status().recorded, 1);
        // The next scheduled run waits a full interval after that run ended.
        sleep(5 * MINUTE + SECOND).await;
        assert_eq!(
            harness.script.starts(&slow, harness.origin),
            [10 * S, 7 * M + 10 * S + 5 * M]
        );

        // A manual run while idle restarts the wait as well.
        harness.script.set(&slow, Behaviour::Takes(SECOND));
        sleep(8 * MINUTE).await;
        let manual_at = (harness.clock)() - harness.origin;
        harness.monitor.run_now(slow.clone()).await.unwrap();
        sleep(10 * MINUTE).await;
        let starts = harness.script.starts(&slow, harness.origin);
        let manual = starts.iter().position(|at| *at == manual_at).unwrap();
        assert_eq!(starts[manual + 1], manual_at + S + 5 * M, "{starts:?}");
        assert_eq!(harness.script.overlaps.load(Ordering::SeqCst), 0);
        harness.monitor.shutdown();
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn pausing_lets_the_run_in_flight_finish_and_schedules_no_other() {
        let path = path();
        let subject = resource(&path);
        supervise(&path, &subject, "http://x.invalid/", Some(5), 15);
        let harness = Harness::launch(path.clone());
        let gate = Arc::new(Notify::new());
        harness
            .script
            .set(&subject, Behaviour::Gate(Arc::clone(&gate)));
        sleep(11 * SECOND).await;
        assert!(harness.check(&subject).running);

        harness.pause(&subject, true);
        let status = harness.check(&subject);
        assert!(status.running, "the run in flight is not interrupted");
        assert_eq!(status.next_run_at, None);
        gate.notify_one();
        sleep(SECOND).await;
        let status = harness.check(&subject);
        assert!(!status.running);
        assert_eq!(status.next_run_at, None);
        assert_eq!(
            harness.monitor.status().recorded,
            1,
            "its result is recorded"
        );
        assert_eq!(harness.script.cancelled.load(Ordering::SeqCst), 0);
        sleep(60 * MINUTE).await;
        assert_eq!(harness.script.starts(&subject, harness.origin).len(), 1);

        // Resuming long after: one run now, not the twelve that were missed, then the rhythm.
        harness
            .script
            .set(&subject, Behaviour::Takes(Duration::ZERO));
        let resumed_at = (harness.clock)() - harness.origin;
        harness.pause(&subject, false);
        sleep(6 * MINUTE).await;
        assert_eq!(
            harness.script.starts(&subject, harness.origin),
            [10 * S, resumed_at, resumed_at + 5 * M]
        );
        // Pausing while idle stops the next run too.
        harness.pause(&subject, true);
        sleep(30 * MINUTE).await;
        assert_eq!(harness.script.starts(&subject, harness.origin).len(), 3);
        harness.monitor.shutdown();
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn removing_a_check_cancels_its_run_in_flight() {
        let path = path();
        let (removed, kept) = (resource(&path), resource(&path));
        supervise(&path, &removed, "http://x.invalid/", Some(5), 15);
        supervise(&path, &kept, "http://y.invalid/", Some(5), 15);
        let harness = Harness::launch(path.clone());
        harness
            .script
            .set(&removed, Behaviour::Gate(Arc::new(Notify::new())));
        sleep(11 * SECOND).await;
        let joined = tokio::spawn({
            let monitor = harness.monitor.clone();
            let subject = removed.clone();
            async move { monitor.run_now(subject).await }
        });
        sleep(SECOND).await;

        remove_http_check(&path, removed.clone()).unwrap();
        harness.monitor.refresh();
        let error = joined.await.unwrap().unwrap_err();
        assert_eq!(error.code, "cancelled");
        assert_eq!(harness.script.cancelled.load(Ordering::SeqCst), 1);
        assert!(harness
            .monitor
            .status()
            .checks
            .iter()
            .all(|item| item.subject != removed));
        sleep(30 * MINUTE).await;
        assert_eq!(harness.script.starts(&removed, harness.origin), [10 * S]);
        // The other check was never disturbed.
        assert_eq!(harness.script.starts(&kept, harness.origin).len(), 6);
        assert_eq!(harness.monitor.status().recorded, 6);

        // Configured again later, it starts afresh.
        harness
            .script
            .set(&removed, Behaviour::Takes(Duration::ZERO));
        supervise(&path, &removed, "http://x.invalid/", Some(5), 15);
        let added_at = (harness.clock)() - harness.origin;
        harness.monitor.refresh();
        sleep(SECOND).await;
        assert_eq!(
            harness.script.starts(&removed, harness.origin),
            [10 * S, added_at]
        );
        harness.monitor.shutdown();
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn a_devhq_failure_is_visible_records_nothing_and_stops_nothing_else() {
        let path = path();
        let (broken, healthy) = (resource(&path), resource(&path));
        supervise(&path, &broken, "http://x.invalid/", Some(5), 15);
        supervise(&path, &healthy, "http://y.invalid/", Some(5), 15);
        let harness = Harness::launch(path.clone());
        harness.script.set(&broken, Behaviour::Fails);
        sleep(31 * MINUTE).await;
        // No retry: a failed run waits a full interval like any other.
        let starts = harness.script.starts(&broken, harness.origin);
        assert!(
            starts.windows(2).all(|pair| pair[1] - pair[0] == 5 * M),
            "{starts:?}"
        );
        assert_eq!(starts.len(), 7);
        assert_eq!(harness.script.starts(&healthy, harness.origin).len(), 7);
        let status = harness.check(&broken);
        let error = status.last_error.unwrap();
        assert_eq!(error.message, "Inventory database error: disk I/O error");
        assert_eq!(harness.at(&Some(error.at)), harness.at(&status.last_run_at));
        assert_eq!(harness.check(&healthy).last_error, None);
        assert_eq!(
            harness.monitor.status().recorded,
            7,
            "only the healthy check recorded"
        );

        // The error stays until a run works again.
        harness
            .script
            .set(&broken, Behaviour::Takes(Duration::ZERO));
        sleep(5 * MINUTE).await;
        assert_eq!(harness.check(&broken).last_error, None);
        harness.monitor.shutdown();
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn an_unreadable_configuration_is_reported_and_read_again() {
        // A directory cannot be opened as the database.
        let directory = std::env::temp_dir().join(format!("devhq-monitor-dir-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let harness = Harness::launch(directory.clone());
        sleep(SECOND).await;
        let status = harness.monitor.status();
        assert!(status
            .error
            .unwrap()
            .starts_with("Inventory database error"));
        assert!(status.checks.is_empty());
        // Reading is tried again on the next pass; nothing ran meanwhile.
        std::fs::remove_dir(&directory).unwrap();
        let subject = resource(&directory);
        supervise(&directory, &subject, "http://x.invalid/", Some(5), 15);
        sleep(31 * SECOND).await;
        assert_eq!(harness.monitor.status().error, None);
        sleep(15 * SECOND).await;
        assert_eq!(harness.script.starts(&subject, harness.origin).len(), 1);
        harness.monitor.shutdown();
        std::fs::remove_file(directory).unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_cancels_runs_in_flight_and_ends_the_loop_at_once() {
        let path = path();
        let (one, two) = (resource(&path), resource(&path));
        supervise(&path, &one, "http://x.invalid/", Some(5), 15);
        supervise(&path, &two, "http://y.invalid/", Some(5), 15);
        let harness = Harness::launch(path.clone());
        for subject in [&one, &two] {
            harness
                .script
                .set(subject, Behaviour::Gate(Arc::new(Notify::new())));
        }
        sleep(21 * SECOND).await;
        assert!(harness.check(&one).running && harness.check(&two).running);
        let joined = tokio::spawn({
            let monitor = harness.monitor.clone();
            let subject = one.clone();
            async move { monitor.run_now(subject).await }
        });
        sleep(SECOND).await;

        let before = Instant::now();
        harness.monitor.shutdown();
        harness.task.await.unwrap();
        assert_eq!(before.elapsed(), Duration::ZERO, "no simulated time passed");
        assert_eq!(joined.await.unwrap().unwrap_err().code, "cancelled");
        assert_eq!(harness.script.cancelled.load(Ordering::SeqCst), 2);
        assert_eq!(
            harness.monitor.run_now(one.clone()).await.unwrap_err().code,
            "cancelled"
        );
        sleep(60 * MINUTE).await;
        assert_eq!(harness.script.starts.lock().unwrap().len(), 2);
        std::fs::remove_file(path).unwrap();
    }

    /// Waits (for real) until the status matches.
    async fn until(monitor: &Monitor, check: impl FnMut(&MonitorStatus) -> bool) -> MonitorStatus {
        let mut receiver = monitor.inner.status.subscribe();
        let status = tokio::time::timeout(Duration::from_secs(15), receiver.wait_for(check))
            .await
            .expect("the monitor did not reach the expected state")
            .map(|status| status.clone());
        status.unwrap()
    }

    fn check_of(status: &MonitorStatus, subject: &Subject) -> CheckStatus {
        status
            .checks
            .iter()
            .find(|item| &item.subject == subject)
            .unwrap()
            .clone()
    }

    /// The real runner against a local server, on a clock the test moves by hand: no waiting
    /// for intervals, only for the requests themselves.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn scheduled_runs_record_the_same_http_evidence_as_manual_ones() {
        let path = path();
        let origin = serve();
        let (web, misconfigured, hanging) = (resource(&path), resource(&path), resource(&path));
        supervise(&path, &web, &format!("{origin}/health"), Some(5), 15);
        supervise(&path, &hanging, &format!("{origin}/hang"), None, 60);
        // A stored configuration that no longer validates: DevHQ's failure, not the system's.
        let Subject::Resource { id } = &misconfigured else {
            unreachable!()
        };
        open(&path)
            .unwrap()
            .execute(
                "INSERT INTO http_checks (resource_id, url, expected_status, timeout_seconds, valid_for_minutes, updated_at, monitor, monitor_interval_minutes)
                 VALUES (?1, 'http://', 200, 5, 60, 0, 'active', 5)",
                [id],
            )
            .unwrap();
        let now = Arc::new(AtomicI64::new(records::now_ms()));
        let clock: Clock = {
            let now = Arc::clone(&now);
            Arc::new(move || now.load(Ordering::SeqCst))
        };
        let monitor = {
            let database = path.clone();
            Monitor::with(
                path.clone(),
                clock,
                Arc::new(move |subject: Subject, lifeline: Lifeline| -> RunFuture {
                    let path = database.clone();
                    Box::pin(
                        async move { http_check::run_http_check(&path, subject, &lifeline).await },
                    )
                }),
            )
        };
        let loop_task = tokio::spawn(monitor.clone().run());
        let advance = |by: i64| {
            now.fetch_add(by, Ordering::SeqCst);
            monitor.refresh();
        };
        until(&monitor, |status| status.checks.len() == 3).await;
        advance(25 * S);
        let status = until(&monitor, |status| {
            status.recorded == 1 && check_of(status, &misconfigured).last_error.is_some()
        })
        .await;
        let error = check_of(&status, &misconfigured).last_error.unwrap();
        assert!(error.message.contains("not valid"), "{}", error.message);
        assert!(check_of(&status, &web).next_run_at.is_some());
        assert!(
            check_of(&status, &misconfigured).next_run_at.is_some(),
            "still supervised"
        );

        let manual = monitor.run_now(web.clone()).await.unwrap();
        let snapshot = list(&path).unwrap();
        assert_eq!(
            snapshot.observations.len(),
            2,
            "the misconfigured check recorded nothing"
        );
        let scheduled = snapshot
            .observations
            .iter()
            .find(|item| item.id != manual.id)
            .unwrap();
        for observation in [scheduled, &manual] {
            assert_eq!(observation.subject, web);
            assert_eq!(observation.source, "http");
            assert_eq!(observation.result, "healthy");
            let evidence = observation.http.as_ref().unwrap();
            assert_eq!(evidence.url, format!("{origin}/health"));
            assert_eq!(
                (evidence.status, evidence.expected_status),
                (Some(200), 200)
            );
            let event = snapshot
                .events
                .iter()
                .find(|item| item.observation_id.as_deref() == Some(observation.id.as_str()))
                .unwrap();
            assert_eq!(
                (event.source.as_str(), event.title.as_str()),
                ("http", "HTTP check: Healthy")
            );
            let validity = time::OffsetDateTime::parse(
                &observation.valid_until,
                &time::format_description::well_known::Rfc3339,
            )
            .unwrap()
                - time::OffsetDateTime::parse(
                    &observation.observed_at,
                    &time::format_description::well_known::Rfc3339,
                )
                .unwrap();
            assert_eq!(validity, time::Duration::minutes(15));
        }

        // Another interval: both supervised checks run again; the failing one did not stop.
        advance(5 * M + S);
        let status = until(&monitor, |status| status.recorded == 3).await;
        assert!(check_of(&status, &misconfigured).last_error.is_some());
        assert_eq!(list(&path).unwrap().observations.len(), 3);

        // Removing a check during a real request cancels it well before its 5 s timeout.
        let running = tokio::spawn({
            let monitor = monitor.clone();
            let subject = hanging.clone();
            async move { monitor.run_now(subject).await }
        });
        until(&monitor, |status| check_of(status, &hanging).running).await;
        // Let the request reach the server, so the removal meets it in flight.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let started = std::time::Instant::now();
        remove_http_check(&path, hanging.clone()).unwrap();
        monitor.refresh();
        assert_eq!(running.await.unwrap().unwrap_err().code, "cancelled");
        assert!(started.elapsed() < Duration::from_secs(1));

        // A run in flight when DevHQ closes is cancelled just as fast. Only this check is
        // left, so no other run can be opening the database while the test cleans up.
        remove_http_check(&path, misconfigured.clone()).unwrap();
        supervise(&path, &web, &format!("{origin}/hang"), Some(5), 15);
        monitor.refresh();
        advance(5 * M + S);
        until(&monitor, |status| check_of(status, &web).running).await;
        let started = std::time::Instant::now();
        monitor.shutdown();
        loop_task.await.unwrap();
        assert!(started.elapsed() < Duration::from_millis(500));
        // Nothing more was recorded: not the hanging request, not the removed check.
        assert_eq!(list(&path).unwrap().observations.len(), 3);
        remove_once_closed(&path);
    }

    /// A run whose request has ended has no await left before its commit, so aborting its task
    /// when DevHQ closes cannot stop it there: its write has to refuse. Without the lifeline
    /// this run recorded its observation, and run_now even returned it, after shutdown.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_run_writing_when_devhq_closes_records_nothing() {
        let path = path();
        let subject = resource(&path);
        supervise(&path, &subject, "http://x.invalid/", None, 15);
        let (entered, writing) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let released = Arc::new(Mutex::new(released));
        let monitor = {
            let database = path.clone();
            Monitor::with(
                path.clone(),
                Arc::new(records::now_ms),
                Arc::new(move |subject: Subject, lifeline: Lifeline| -> RunFuture {
                    let (path, entered) = (database.clone(), entered.clone());
                    let released = Arc::clone(&released);
                    Box::pin(async move {
                        // The request has ended: from here to the commit nothing awaits.
                        entered.send(()).unwrap();
                        released.lock().unwrap().recv().unwrap();
                        let evidence = HttpEvidence {
                            url: "http://x.invalid/".into(),
                            expected_status: 200,
                            status: Some(200),
                            latency_ms: 1,
                            error: None,
                            redirects: Vec::new(),
                        };
                        let now = records::now_ms();
                        records::store_observation(
                            &path,
                            NewObservation {
                                subject: &subject,
                                result: "healthy",
                                summary: "HTTP 200 as expected, in 1 ms",
                                observed_at: now,
                                valid_for_minutes: 15,
                                recorded_at: now,
                                http: Some(&evidence),
                                reporter: None,
                                lifeline: Some(&lifeline),
                            },
                        )
                    })
                }),
            )
        };
        let run = tokio::spawn({
            let monitor = monitor.clone();
            async move { monitor.run_now(subject).await }
        });
        tokio::task::spawn_blocking(move || writing.recv().unwrap())
            .await
            .unwrap();

        monitor.shutdown();
        release.send(()).unwrap();
        let result = run.await.unwrap();
        assert_eq!(result.map(|_| ()).unwrap_err().code, "cancelled");
        assert!(list(&path).unwrap().observations.is_empty());
        remove_once_closed(&path);
    }

    /// Removes a test database once no connection has it open. The run cancelled above may
    /// have been reading its configuration on a worker thread when DevHQ shut down: a
    /// cancellation only takes effect at the run's next await, so its connection can close just
    /// after the loop has ended. Linux removes an open file anyway; Windows refuses (error 32,
    /// a sharing violation), so wait for the connection, but not forever: a connection that is
    /// never closed is a bug.
    fn remove_once_closed(path: &Path) {
        const SHARING_VIOLATION: i32 = 32;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match std::fs::remove_file(path) {
                Ok(()) => return,
                Err(error)
                    if error.raw_os_error() == Some(SHARING_VIOLATION)
                        && cfg!(windows)
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Err(error) => panic!("cannot remove {}: {error}", path.display()),
            }
        }
    }
}
