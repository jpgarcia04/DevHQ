//! DevHQ's GitHub reader (`docs/leer-github.md`): what GitHub says of a repository the inventory
//! names, read with the person's fine-grained, read-only token (`credentials`).
//!
//! It asks three things and nothing else, always of `api.github.com` over HTTPS with the
//! certificate verified, and nothing GitHub answers decides where a request goes: redirects are not
//! followed, the owner and name come from the inventory and the branch GitHub declared travels only
//! percent-encoded in a query.
//!
//! 1. The repository, for the default branch it declares.
//! 2. The newest 100 finished runs on that branch, from which it keeps, for each workflow, the newest
//!    run that concluded something: a cancelled, skipped or stale run says nothing about the branch,
//!    so the one before it counts. A workflow whose every run among them said nothing is not kept.
//! 3. The open pull requests, at most 100.
//!
//! Every answer is untrusted data: its size is capped while it arrives, its shape is checked field
//! by field and any element DevHQ cannot accept refuses the whole answer, never part of it. A title
//! or a workflow's name is kept as plain text, and every address must lead into the repository read.
//! GitHub's own refusals (401, 403, 404) are kept as `refused`; anything else that kept DevHQ from
//! reading (no answer, a limit, an answer it refused) as `unreadable`.

pub mod readings;

use crate::credentials::Secret;
use crate::installations::plain_text;
use crate::records::{format_time, parse_time};
use reqwest::header::{HeaderValue, ACCEPT, AUTHORIZATION};
use reqwest::{redirect, Client};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::future::Future;
use std::time::Duration;

/// DevHQ's reader and the version of GitHub's REST API it asks for, kept with every reading.
pub const READER: &str = "DevHQ GitHub reader 1 (REST 2022-11-28)";
const API: &str = "https://api.github.com";
const API_VERSION: &str = "2022-11-28";
/// The most of one answer DevHQ takes: 100 runs with their repository objects are about 1 MiB.
pub(crate) const MAX_ANSWER: usize = 8 * 1024 * 1024;
/// Runs and pull requests asked for, and kept, per reading.
pub(crate) const MAX_ITEMS: usize = 100;
/// The time one repository's three requests may take together.
pub(crate) const DEADLINE: Duration = Duration::from_secs(30);
const MAX_NAME: usize = 120;
const MAX_TITLE: usize = 256;
const MAX_BRANCH: usize = 255;
const MAX_ADDRESS: usize = 2048;

/// Every conclusion GitHub gives a finished run.
const CONCLUSIONS: [&str; 9] = [
    "success",
    "failure",
    "neutral",
    "cancelled",
    "skipped",
    "timed_out",
    "action_required",
    "startup_failure",
    "stale",
];
/// The conclusions that say nothing about the branch: the run before counts.
const SAYS_NOTHING: [&str; 3] = ["cancelled", "skipped", "stale"];
/// The conclusions of a run that did not pass: what `main_failing` looks for (see `pending`).
pub(crate) const FAILED: [&str; 3] = ["failure", "timed_out", "startup_failure"];

/// The owner and name of a repository the inventory names as `https://github.com/<owner>/<name>`,
/// with nothing after it but, as a clone address has it, `.git`; None for any other address, which
/// DevHQ does not read on GitHub.
pub(crate) fn repository(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("https://github.com/")?;
    let (owner, name) = rest.split_once('/')?;
    let name = name.strip_suffix(".git").unwrap_or(name);
    let owner_ok = (1..=39).contains(&owner.len())
        && owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
    let name_ok = (1..=100).contains(&name.len())
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    (owner_ok && name_ok).then(|| (owner.to_owned(), name.to_owned()))
}

/// The newest run that concluded something of one workflow on the default branch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Workflow {
    /// The workflow's name, as plain text.
    pub workflow: String,
    /// One of `CONCLUSIONS` but those in `SAYS_NOTHING`.
    pub conclusion: String,
    /// The commit it ran on: 40 hexadecimal digits.
    pub commit: String,
    /// When GitHub last updated the run, which for a finished run is when it finished.
    pub finished_at: String,
    /// The run on GitHub.
    pub address: String,
}

/// An open pull request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PullRequest {
    pub number: i64,
    /// Its title, as plain text.
    pub title: String,
    pub draft: bool,
    /// When it last changed: a commit, a comment, a review, a label.
    pub updated_at: String,
    pub address: String,
}

/// What a reading found, as it is kept (see `readings::store`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// `read`, `unreadable` or `refused`.
    pub outcome: &'static str,
    /// Why it was not read; only then.
    pub detail: Option<String>,
    pub default_branch: Option<String>,
    pub workflows: Vec<Workflow>,
    /// GitHub had more finished runs on the branch than DevHQ looked at, so a workflow that has not
    /// run among the newest 100 is missing.
    pub workflows_more: bool,
    pub pull_requests: Vec<PullRequest>,
    /// GitHub had more open pull requests than the 100 kept.
    pub pull_requests_more: bool,
}

impl Facts {
    fn not_read(outcome: &'static str, detail: String) -> Self {
        Facts {
            outcome,
            detail: Some(plain_text(&detail, 1000)),
            default_branch: None,
            workflows: Vec::new(),
            workflows_more: false,
            pull_requests: Vec::new(),
            pull_requests_more: false,
        }
    }
}

/// What GitHub answered to one request.
#[derive(Debug)]
pub struct Answer {
    pub status: u16,
    /// Whether GitHub said the token has no requests left this hour.
    pub rate_limited: bool,
    pub body: Vec<u8>,
}

/// Asks GitHub's API for a path under `api.github.com`: the real client or, in tests, invented
/// and hostile answers. An error is what kept an answer from arriving, in a few words.
pub trait Fetch {
    fn get(&self, path: &str) -> impl Future<Output = Result<Answer, String>> + Send;
}

/// The real client: HTTPS to `api.github.com` with the token, no redirects, every answer capped.
pub struct Https {
    client: Client,
    authorization: HeaderValue,
}

impl Https {
    pub fn new(token: &Secret) -> Result<Self, String> {
        let mut bearer = b"Bearer ".to_vec();
        bearer.extend_from_slice(token.bytes());
        let header = HeaderValue::from_bytes(&bearer);
        bearer.fill(0);
        let mut authorization =
            header.map_err(|_| "the kept token cannot be sent as a header".to_owned())?;
        // Never printed by reqwest's own debugging.
        authorization.set_sensitive(true);
        let client = Client::builder()
            .redirect(redirect::Policy::none())
            .https_only(true)
            .connect_timeout(Duration::from_secs(10))
            .timeout(DEADLINE)
            .user_agent(concat!(
                "DevHQ/",
                env!("CARGO_PKG_VERSION"),
                " (GitHub reader)"
            ))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Https {
            client,
            authorization,
        })
    }
}

impl Fetch for Https {
    async fn get(&self, path: &str) -> Result<Answer, String> {
        let mut response = self
            .client
            .get(format!("{API}{path}"))
            .header(AUTHORIZATION, self.authorization.clone())
            .header(ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .send()
            .await
            .map_err(|error| error.without_url().to_string())?;
        let status = response.status().as_u16();
        let rate_limited = response
            .headers()
            .get("x-ratelimit-remaining")
            .is_some_and(|value| value.as_bytes() == b"0");
        if response
            .content_length()
            .is_some_and(|length| length > MAX_ANSWER as u64)
        {
            return Err(format!("an answer larger than {} MiB", MAX_ANSWER >> 20));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| error.without_url().to_string())?
        {
            if body.len() + chunk.len() > MAX_ANSWER {
                return Err(format!("an answer larger than {} MiB", MAX_ANSWER >> 20));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Answer {
            status,
            rate_limited,
            body,
        })
    }
}

/// Percent-encodes everything but unreserved characters, so that a branch's name can only ever be
/// one value of a query.
fn encoded(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

/// Why one request read nothing.
enum Stop {
    Unreadable(String),
    Refused(String),
}

impl Stop {
    fn facts(self) -> Facts {
        match self {
            Stop::Unreadable(detail) => Facts::not_read("unreadable", detail),
            Stop::Refused(detail) => Facts::not_read("refused", detail),
        }
    }
}

fn refusal() -> Stop {
    Stop::Unreadable("GitHub's answer is not what DevHQ expects.".into())
}

/// The JSON of a successful answer to `what`, or why there is none.
async fn ask(fetch: &impl Fetch, path: &str, what: &str) -> Result<Value, Stop> {
    let answer = fetch
        .get(path)
        .await
        .map_err(|error| Stop::Unreadable(format!("GitHub did not answer for {what}: {error}.")))?;
    if answer.body.len() > MAX_ANSWER {
        return Err(Stop::Unreadable(format!(
            "GitHub's answer for {what} is larger than {} MiB.",
            MAX_ANSWER >> 20
        )));
    }
    match answer.status {
        200 => serde_json::from_slice(&answer.body).map_err(|_| refusal()),
        403 | 429 if answer.rate_limited => Err(Stop::Unreadable(
            "The token has no GitHub requests left this hour. Read again later.".into(),
        )),
        401 => Err(Stop::Refused(
            "GitHub refused the token (401): it expired, was revoked or is not valid. Save a new one in Settings.".into(),
        )),
        403 => Err(Stop::Refused(format!(
            "GitHub refused {what} to the token (403): give it read access to this repository's Metadata, Actions and Pull requests."
        ))),
        404 => Err(Stop::Refused(format!(
            "GitHub answered that {what} does not exist or that the token cannot see it (404)."
        ))),
        301 | 302 | 307 | 308 => Err(Stop::Unreadable(format!(
            "GitHub answered that the repository moved ({}): update the reference with its new address.",
            answer.status
        ))),
        status => Err(Stop::Unreadable(format!(
            "GitHub answered {status} for {what}."
        ))),
    }
}

/// A text field of an untrusted object.
fn text<'a>(object: &'a Value, field: &str) -> Result<&'a str, Stop> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(refusal)
}

/// A time GitHub gave, as DevHQ writes times.
fn time(object: &Value, field: &str) -> Result<String, Stop> {
    let ms = parse_time(text(object, field)?, field).map_err(|_| refusal())?;
    format_time(ms).ok_or_else(refusal)
}

/// An address GitHub gave, accepted only inside the repository read, under `kind` (`actions/runs`
/// or `pull`), ignoring the case GitHub writes the owner and name in.
fn address(object: &Value, owner: &str, name: &str, kind: &str) -> Result<String, Stop> {
    let address = text(object, "html_url")?;
    let prefix = format!("https://github.com/{owner}/{name}/{kind}/");
    let inside = address.len() <= MAX_ADDRESS
        && address.is_char_boundary(prefix.len())
        && address.len() > prefix.len()
        && address[..prefix.len()].eq_ignore_ascii_case(&prefix)
        && address[prefix.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_digit());
    inside.then(|| address.to_owned()).ok_or_else(refusal)
}

/// A list of at most `MAX_ITEMS` objects.
fn items(value: &Value) -> Result<&Vec<Value>, Stop> {
    value
        .as_array()
        .filter(|items| items.len() <= MAX_ITEMS && items.iter().all(Value::is_object))
        .ok_or_else(refusal)
}

fn default_branch(repository: &Value, owner: &str, name: &str) -> Result<String, Stop> {
    let full_name = text(repository, "full_name")?;
    if !full_name.eq_ignore_ascii_case(&format!("{owner}/{name}")) {
        return Err(refusal());
    }
    let branch = text(repository, "default_branch")?;
    if branch.is_empty()
        || branch.len() > MAX_BRANCH
        || branch.chars().any(char::is_control)
        || branch.trim() != branch
    {
        return Err(refusal());
    }
    Ok(branch.to_owned())
}

/// Each workflow's newest run that concluded something, in the order GitHub gave them (newest
/// first), and whether GitHub had more runs than it gave.
fn workflows(
    answer: &Value,
    owner: &str,
    name: &str,
    branch: &str,
) -> Result<(Vec<Workflow>, bool), Stop> {
    let total = answer
        .get("total_count")
        .and_then(Value::as_u64)
        .ok_or_else(refusal)?;
    let runs = items(answer.get("workflow_runs").ok_or_else(refusal)?)?;
    let mut seen = Vec::new();
    let mut kept = Vec::new();
    for run in runs {
        let id = run
            .get("workflow_id")
            .and_then(Value::as_u64)
            .ok_or_else(refusal)?;
        let conclusion = text(run, "conclusion")?;
        let commit = text(run, "head_sha")?;
        if !CONCLUSIONS.contains(&conclusion)
            || text(run, "status")? != "completed"
            || text(run, "head_branch")? != branch
            || commit.len() != 40
            || !commit.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(refusal());
        }
        let workflow = Workflow {
            workflow: match plain_text(text(run, "name")?, MAX_NAME) {
                empty if empty.is_empty() => "(unnamed workflow)".to_owned(),
                named => named,
            },
            conclusion: conclusion.to_owned(),
            commit: commit.to_ascii_lowercase(),
            finished_at: time(run, "updated_at")?,
            address: address(run, owner, name, "actions/runs")?,
        };
        // Every run is checked, even of a workflow already kept: a hostile element refuses all.
        if SAYS_NOTHING.contains(&conclusion) || seen.contains(&id) {
            continue;
        }
        seen.push(id);
        kept.push(workflow);
    }
    Ok((kept, total > runs.len() as u64))
}

fn pull_requests(answer: &Value, owner: &str, name: &str) -> Result<Vec<PullRequest>, Stop> {
    items(answer)?
        .iter()
        .map(|pull| {
            let number = pull
                .get("number")
                .and_then(Value::as_i64)
                .filter(|number| *number >= 1)
                .ok_or_else(refusal)?;
            let address = address(pull, owner, name, "pull")?;
            if !address.ends_with(&format!("/pull/{number}")) {
                return Err(refusal());
            }
            Ok(PullRequest {
                number,
                title: match plain_text(text(pull, "title")?, MAX_TITLE) {
                    empty if empty.is_empty() => "(untitled)".to_owned(),
                    titled => titled,
                },
                draft: pull
                    .get("draft")
                    .and_then(Value::as_bool)
                    .ok_or_else(refusal)?,
                updated_at: time(pull, "updated_at")?,
                address,
            })
        })
        .collect()
}

async fn read_all(fetch: &impl Fetch, owner: &str, name: &str) -> Result<Facts, Stop> {
    let base = format!("/repos/{owner}/{name}");
    let repository = ask(fetch, &base, "the repository").await?;
    let branch = default_branch(&repository, owner, name)?;
    let runs = ask(
        fetch,
        &format!(
            "{base}/actions/runs?branch={}&status=completed&per_page={MAX_ITEMS}",
            encoded(&branch)
        ),
        "the repository's workflow runs",
    )
    .await?;
    let (workflows, workflows_more) = workflows(&runs, owner, name, &branch)?;
    let pulls = ask(
        fetch,
        &format!("{base}/pulls?state=open&per_page={MAX_ITEMS}"),
        "the repository's pull requests",
    )
    .await?;
    let pull_requests = pull_requests(&pulls, owner, name)?;
    // A full page may hide more; GitHub does not count open pull requests in this answer.
    let pull_requests_more = pull_requests.len() == MAX_ITEMS;
    Ok(Facts {
        outcome: "read",
        detail: None,
        default_branch: Some(branch),
        workflows,
        workflows_more,
        pull_requests,
        pull_requests_more,
    })
}

/// Reads what GitHub says of `owner/name` within `DEADLINE`, never failing: what kept DevHQ from
/// reading it is the reading's outcome.
pub async fn read(fetch: &impl Fetch, owner: &str, name: &str) -> Facts {
    match tokio::time::timeout(DEADLINE, read_all(fetch, owner, name)).await {
        Ok(Ok(facts)) => facts,
        Ok(Err(stop)) => stop.facts(),
        Err(_) => Facts::not_read(
            "unreadable",
            format!("GitHub did not answer within {} s.", DEADLINE.as_secs()),
        ),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// Invented answers by path, recording every path asked. A path with no answer gets a 500.
    #[derive(Default)]
    pub(crate) struct Invented {
        pub answers: BTreeMap<String, (u16, Vec<u8>)>,
        pub asked: Mutex<Vec<String>>,
        pub rate_limited: bool,
    }

    impl Invented {
        pub(crate) fn answer(mut self, path: &str, status: u16, body: Value) -> Self {
            self.answers
                .insert(path.to_owned(), (status, body.to_string().into_bytes()));
            self
        }
        fn raw(mut self, path: &str, status: u16, body: Vec<u8>) -> Self {
            self.answers.insert(path.to_owned(), (status, body));
            self
        }
    }

    impl Fetch for Invented {
        async fn get(&self, path: &str) -> Result<Answer, String> {
            self.asked.lock().unwrap().push(path.to_owned());
            let (status, body) = self
                .answers
                .get(path)
                .cloned()
                .unwrap_or((500, b"{}".to_vec()));
            Ok(Answer {
                status,
                rate_limited: self.rate_limited,
                body,
            })
        }
    }

    pub(crate) const REPO: &str = "/repos/ana-ruiz/shop";
    pub(crate) const RUNS: &str =
        "/repos/ana-ruiz/shop/actions/runs?branch=main&status=completed&per_page=100";
    pub(crate) const PULLS: &str = "/repos/ana-ruiz/shop/pulls?state=open&per_page=100";

    pub(crate) fn run(workflow: u64, name: &str, conclusion: &str, id: u64) -> Value {
        json!({
            "id": id,
            "workflow_id": workflow,
            "name": name,
            "status": "completed",
            "conclusion": conclusion,
            "head_branch": "main",
            "head_sha": format!("{:040x}", id),
            "updated_at": "2026-11-20T10:00:00Z",
            "html_url": format!("https://github.com/ana-ruiz/shop/actions/runs/{id}"),
        })
    }

    pub(crate) fn pull(number: i64, title: &str, updated_at: &str) -> Value {
        json!({
            "number": number,
            "title": title,
            "draft": false,
            "updated_at": updated_at,
            "html_url": format!("https://github.com/ana-ruiz/shop/pull/{number}"),
        })
    }

    /// An invented repository with two workflows, one failing after a cancelled run, and one open
    /// pull request.
    pub(crate) fn shop() -> Invented {
        Invented::default()
            .answer(
                REPO,
                200,
                json!({"full_name": "Ana-Ruiz/shop", "default_branch": "main"}),
            )
            .answer(
                RUNS,
                200,
                json!({"total_count": 4, "workflow_runs": [
                    run(1, "CI", "cancelled", 9),
                    run(1, "CI", "failure", 8),
                    run(2, "Scheduled audit", "success", 7),
                    run(1, "CI", "success", 6),
                ]}),
            )
            .answer(
                PULLS,
                200,
                json!([pull(
                    12,
                    "Audit\u{0007} dependencies",
                    "2026-11-10T08:00:00Z"
                )]),
            )
    }

    fn read_now(fetch: &Invented) -> Facts {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(read(fetch, "ana-ruiz", "shop"))
    }

    #[test]
    fn names_only_plain_repositories_on_github() {
        assert_eq!(
            repository("https://github.com/ana-ruiz/shop.web"),
            Some(("ana-ruiz".into(), "shop.web".into()))
        );
        assert_eq!(
            repository("https://github.com/ana-ruiz/shop.git"),
            Some(("ana-ruiz".into(), "shop".into()))
        );
        for other in [
            "https://github.com/ana-ruiz",
            "https://github.com/ana-ruiz/shop/",
            "https://github.com/ana-ruiz/shop/tree/main",
            "https://github.com/ana-ruiz/shop?tab=readme",
            "https://github.com/ana-ruiz/shop#readme",
            "http://github.com/ana-ruiz/shop",
            "https://github.com.example/ana-ruiz/shop",
            "https://gitlab.example/ana-ruiz/shop",
            "https://github.com/ana_ruiz/shop",
            "https://github.com/ana-ruiz/..",
            "https://github.com/ana-ruiz/.git",
            "https://github.com/ana-ruiz/sh%6Fp",
        ] {
            assert_eq!(repository(other), None, "{other}");
        }
    }

    #[test]
    fn keeps_each_workflows_newest_run_that_concluded_something() {
        let fetch = shop();
        let facts = read_now(&fetch);
        assert_eq!(facts.outcome, "read");
        assert_eq!(facts.detail, None);
        assert_eq!(facts.default_branch.as_deref(), Some("main"));
        assert_eq!(
            facts
                .workflows
                .iter()
                .map(|workflow| (workflow.workflow.as_str(), workflow.conclusion.as_str()))
                .collect::<Vec<_>>(),
            [("CI", "failure"), ("Scheduled audit", "success")]
        );
        assert_eq!(facts.workflows[0].commit, format!("{:040x}", 8));
        assert_eq!(facts.workflows[0].finished_at, "2026-11-20T10:00:00Z");
        assert!(!facts.workflows_more);
        assert_eq!(facts.pull_requests.len(), 1);
        // A control character in a title is kept as a space, never as itself.
        assert_eq!(facts.pull_requests[0].title, "Audit  dependencies");
        assert!(!facts.pull_requests_more);
        assert_eq!(*fetch.asked.lock().unwrap(), [REPO, RUNS, PULLS]);
    }

    #[test]
    fn a_branch_name_travels_only_as_one_encoded_value() {
        let fetch = Invented::default().answer(
            REPO,
            200,
            json!({"full_name": "ana-ruiz/shop", "default_branch": "x&per_page=1/../../evil#"}),
        );
        read_now(&fetch);
        assert_eq!(
            fetch.asked.lock().unwrap()[1],
            "/repos/ana-ruiz/shop/actions/runs?branch=x%26per_page%3D1%2F..%2F..%2Fevil%23&status=completed&per_page=100"
        );
    }

    #[test]
    fn says_when_github_had_more_than_it_looked_at() {
        let runs: Vec<Value> = (0..100).map(|id| run(1, "CI", "success", id + 1)).collect();
        let pulls: Vec<Value> = (1..=100)
            .map(|number| pull(number, "Change", "2026-11-20T10:00:00Z"))
            .collect();
        let fetch = shop()
            .answer(
                RUNS,
                200,
                json!({"total_count": 250, "workflow_runs": runs}),
            )
            .answer(PULLS, 200, json!(pulls));
        let facts = read_now(&fetch);
        assert_eq!(facts.outcome, "read");
        assert_eq!(facts.workflows.len(), 1);
        assert!(facts.workflows_more);
        assert!(facts.pull_requests_more);
    }

    #[test]
    fn keeps_githubs_refusals_as_refused_and_the_rest_as_unreadable() {
        let cases = [
            (shop().answer(REPO, 401, json!({})), "refused", "(401)"),
            (shop().answer(REPO, 404, json!({})), "refused", "(404)"),
            (shop().answer(RUNS, 403, json!({})), "refused", "Actions"),
            (shop().answer(REPO, 301, json!({})), "unreadable", "moved"),
            (shop().answer(PULLS, 502, json!({})), "unreadable", "502"),
            (
                Invented {
                    rate_limited: true,
                    ..shop().answer(REPO, 403, json!({}))
                },
                "unreadable",
                "requests left",
            ),
        ];
        for (fetch, outcome, says) in cases {
            let facts = read_now(&fetch);
            assert_eq!(facts.outcome, outcome);
            assert!(
                facts.detail.as_deref().unwrap().contains(says),
                "{:?}",
                facts.detail
            );
            assert_eq!(facts.default_branch, None);
            assert!(facts.workflows.is_empty() && facts.pull_requests.is_empty());
        }
    }

    #[test]
    fn refuses_a_whole_answer_for_one_hostile_element() {
        let mut bad_run = run(1, "CI", "failure", 8);
        let hostile: Vec<(&str, Invented)> = vec![
            ("not JSON", shop().raw(REPO, 200, b"<html>".to_vec())),
            (
                "another repository",
                shop().answer(
                    REPO,
                    200,
                    json!({"full_name": "mallory/shop", "default_branch": "main"}),
                ),
            ),
            (
                "a branch with a line break",
                shop().answer(
                    REPO,
                    200,
                    json!({"full_name": "ana-ruiz/shop", "default_branch": "ma\nin"}),
                ),
            ),
            (
                "a missing field",
                shop().answer(REPO, 200, json!({"full_name": "ana-ruiz/shop"})),
            ),
            ("a list instead of runs", shop().answer(RUNS, 200, json!([]))),
            ("too many runs", {
                let runs: Vec<Value> = (0..101).map(|id| run(1, "CI", "success", id + 1)).collect();
                shop().answer(RUNS, 200, json!({"total_count": 101, "workflow_runs": runs}))
            }),
            ("an unknown conclusion", {
                bad_run["conclusion"] = json!("exploded");
                shop().answer(RUNS, 200, json!({"total_count": 1, "workflow_runs": [bad_run.clone()]}))
            }),
            ("a run of another branch", {
                let mut other = run(1, "CI", "failure", 8);
                other["head_branch"] = json!("feature");
                shop().answer(RUNS, 200, json!({"total_count": 1, "workflow_runs": [other]}))
            }),
            ("a short commit", {
                let mut other = run(1, "CI", "failure", 8);
                other["head_sha"] = json!("abc123");
                shop().answer(RUNS, 200, json!({"total_count": 1, "workflow_runs": [other]}))
            }),
            ("an address elsewhere", {
                let mut other = run(1, "CI", "failure", 8);
                other["html_url"] = json!("https://evil.example/ana-ruiz/shop/actions/runs/8");
                shop().answer(RUNS, 200, json!({"total_count": 1, "workflow_runs": [other]}))
            }),
            ("an address in another repository", {
                let mut other = run(1, "CI", "failure", 8);
                other["html_url"] = json!("https://github.com/ana-ruiz/shopping/actions/runs/8");
                shop().answer(RUNS, 200, json!({"total_count": 1, "workflow_runs": [other]}))
            }),
            (
                "a hostile element after a good one",
                shop().answer(
                    RUNS,
                    200,
                    json!({"total_count": 2, "workflow_runs": [run(1, "CI", "failure", 8), {"workflow_id": "1"}]}),
                ),
            ),
            (
                "a time without a zone",
                shop().answer(PULLS, 200, json!([pull(12, "Change", "2026-11-10 08:00")])),
            ),
            ("a pull request that is not its address", {
                let mut other = pull(12, "Change", "2026-11-10T08:00:00Z");
                other["number"] = json!(13);
                shop().answer(PULLS, 200, json!([other]))
            }),
            (
                "a number as text",
                shop().answer(
                    PULLS,
                    200,
                    json!([{"number": "12", "title": "x", "draft": false, "updated_at": "2026-11-10T08:00:00Z", "html_url": "https://github.com/ana-ruiz/shop/pull/12"}]),
                ),
            ),
            (
                "an answer too large",
                shop().raw(PULLS, 200, vec![b' '; MAX_ANSWER + 1]),
            ),
        ];
        for (what, fetch) in hostile {
            let facts = read_now(&fetch);
            assert_eq!(facts.outcome, "unreadable", "{what}");
            assert!(
                facts.workflows.is_empty() && facts.pull_requests.is_empty(),
                "{what}"
            );
            assert_eq!(facts.default_branch, None, "{what}");
        }
    }

    #[test]
    fn a_long_title_is_cut_and_an_empty_one_named() {
        let long = "x".repeat(5000);
        let fetch = shop().answer(
            PULLS,
            200,
            json!([
                pull(1, &long, "2026-11-10T08:00:00Z"),
                pull(2, "\u{0000}", "2026-11-10T08:00:00Z")
            ]),
        );
        let facts = read_now(&fetch);
        assert_eq!(facts.pull_requests[0].title.chars().count(), MAX_TITLE);
        assert_eq!(facts.pull_requests[1].title, "(untitled)");
    }

    #[test]
    fn a_github_that_never_answers_is_unreadable_at_the_deadline() {
        struct Silent;
        impl Fetch for Silent {
            async fn get(&self, _: &str) -> Result<Answer, String> {
                std::future::pending().await
            }
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap();
        let facts = runtime.block_on(read(&Silent, "ana-ruiz", "shop"));
        assert_eq!(facts.outcome, "unreadable");
        assert!(facts.detail.unwrap().contains("30 s"));
    }

    #[test]
    fn the_client_sends_the_token_as_a_sensitive_header_only() {
        let token = crate::credentials::tests::TOKEN;
        let https = Https::new(&crate::credentials::Secret::for_tests(token)).unwrap();
        assert!(https.authorization.is_sensitive());
        assert!(!format!("{:?}", https.authorization).contains("github_pat_"));
    }
}
