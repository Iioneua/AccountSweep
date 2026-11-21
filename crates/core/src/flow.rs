//! The workflow state machine behind `accountsweep next`.
//!
//! The tool, not the agent, decides what happens next. `next` is read-only: it
//! looks at what is stored and returns exactly one instruction (`next.v1`).
//! Answers, ingests and skips are what move the sweep forward.
//!
//! M1 covers: consent → sources → Gmail discovery → summary.

use serde::Serialize;

use crate::error::{Error, Result};
use crate::output::ErrorCode;
use crate::rules::{Query, rules};
use crate::store::{Store, Summary};

/// Answer key for the Basic-mode consent.
pub const CONSENT_KEY: &str = "consent";
/// Answer key for the chosen sources.
pub const SOURCES_KEY: &str = "sources";
/// Answer key recorded when the user or agent skips Gmail.
pub const GMAIL_SKIPPED_KEY: &str = "gmail_skipped";

/// Value of the `schema` field in every instruction.
pub const NEXT_SCHEMA: &str = "accountsweep.next.v1";

/// Fields the agent may pass on from Gmail. Nothing else.
pub const GMAIL_FIELDS: [&str; 4] = ["message_id", "from", "subject", "date"];

/// The consent text shown before anything is read.
pub const CONSENT_TEXT: &str = "Before I start: in Basic mode I (your AI agent) will search your Gmail and read the sender, subject and date of matching emails (sign-ups, receipts, password resets, security alerts). That information passes through your AI provider. The AccountSweep tool runs on this computer, stores only a summary locally, and never sends your data to its authors. Do you agree?";

/// Workflow phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Consent and setup.
    Setup,
    /// Choosing sources.
    Sources,
    /// Reading sources.
    Discover,
    /// Final report.
    Report,
}

/// Kind of answer the user gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    /// One option.
    Choice,
    /// Several options, comma-separated.
    MultiChoice,
    /// yes or no.
    YesNo,
}

/// One option of a question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Choice {
    /// Value to pass to `accountsweep answer`.
    pub value: &'static str,
    /// What to show the user.
    pub label: &'static str,
}

/// One Gmail search for the agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueryOut {
    /// Query id (goes into the ingest payload).
    pub id: String,
    /// Gmail search syntax.
    pub q: String,
    /// Result cap.
    pub max_results: u32,
}

impl From<&Query> for QueryOut {
    fn from(q: &Query) -> Self {
        QueryOut {
            id: q.id.clone(),
            q: q.q.clone(),
            max_results: q.max_results,
        }
    }
}

/// What the agent should do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Step {
    /// Ask the user a question prepared by the tool.
    AskUser {
        /// Answer key.
        key: &'static str,
        /// Question text.
        say: String,
        /// Kind of answer.
        input: InputKind,
        /// Options (for choices).
        #[serde(skip_serializing_if = "Vec::is_empty")]
        options: Vec<Choice>,
        /// Command to record the answer (append the value).
        answer_with: Vec<String>,
    },
    /// Run Gmail searches with the agent's connector and pipe the results in.
    ConnectorRead {
        /// Connector to use.
        connector: &'static str,
        /// Searches still to run.
        queries: Vec<QueryOut>,
        /// The only fields to pass on.
        fields: [&'static str; 4],
        /// Command that receives each query's results on stdin.
        ingest_with: Vec<String>,
        /// Short guidance for the agent.
        say: String,
    },
    /// The sweep is complete; relay the summary.
    Done {
        /// Summary for the user.
        say: String,
    },
}

/// One instruction (`next.v1`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Instruction {
    schema: &'static str,
    /// Workflow phase.
    pub phase: Phase,
    /// What to do.
    #[serde(flatten)]
    pub step: Step,
}

impl Instruction {
    fn new(phase: Phase, step: Step) -> Self {
        Instruction {
            schema: NEXT_SCHEMA,
            phase,
            step,
        }
    }

    /// The text to show the user, whatever the step type.
    pub fn say(&self) -> &str {
        match &self.step {
            Step::AskUser { say, .. } | Step::ConnectorRead { say, .. } | Step::Done { say } => say,
        }
    }
}

fn cmd(args: &[&str]) -> Vec<String> {
    std::iter::once("accountsweep")
        .chain(args.iter().copied())
        .map(String::from)
        .collect()
}

/// Gmail queries not yet ingested, in pack order.
pub fn pending_queries(store: &Store) -> Result<Vec<&'static Query>> {
    let done = store.completed_queries()?;
    Ok(rules()
        .queries
        .iter()
        .filter(|q| !done.contains(&q.id))
        .collect())
}

const SOURCE_CHOICES: [Choice; 1] = [Choice {
    value: "gmail",
    label: "My Gmail (sign-ups, receipts, password resets, security alerts)",
}];

/// Compute the next instruction. Read-only.
pub fn next(store: &Store) -> Result<Instruction> {
    match store.answer(CONSENT_KEY)?.as_deref() {
        None => {
            return Ok(Instruction::new(
                Phase::Setup,
                Step::AskUser {
                    key: CONSENT_KEY,
                    say: CONSENT_TEXT.into(),
                    input: InputKind::YesNo,
                    options: Vec::new(),
                    answer_with: cmd(&["answer", CONSENT_KEY]),
                },
            ));
        }
        Some("yes") => {}
        Some(_) => {
            return Ok(Instruction::new(
                Phase::Report,
                Step::Done {
                    say: "Understood: nothing was read. Basic mode needs your agreement because your AI agent reads the email information. If you change your mind, run `accountsweep consent --reset` and start again.".into(),
                },
            ));
        }
    }

    let Some(sources) = store.answer(SOURCES_KEY)? else {
        return Ok(Instruction::new(
            Phase::Sources,
            Step::AskUser {
                key: SOURCES_KEY,
                say: "What should I check? (Connected apps and devices pages come in a later version.)".into(),
                input: InputKind::MultiChoice,
                options: SOURCE_CHOICES.to_vec(),
                answer_with: cmd(&["answer", SOURCES_KEY]),
            },
        ));
    };

    let gmail_selected = sources.split(',').any(|s| s.trim() == "gmail");
    if gmail_selected && store.answer(GMAIL_SKIPPED_KEY)?.is_none() {
        let pending = pending_queries(store)?;
        if !pending.is_empty() {
            return Ok(Instruction::new(
                Phase::Discover,
                Step::ConnectorRead {
                    connector: "gmail",
                    queries: pending.into_iter().map(QueryOut::from).collect(),
                    fields: GMAIL_FIELDS,
                    ingest_with: cmd(&["ingest", "--kind", "gmail_headers"]),
                    say: "Run each search with your Gmail connector. For each one, pipe {\"schema\":\"accountsweep.ingest.gmail_headers.v1\",\"query_id\":<id>,\"items\":[...]} with only the listed fields (an empty items list if nothing matched). No Gmail connector? Run `accountsweep skip gmail`.".into(),
                },
            ));
        }
    }

    Ok(Instruction::new(
        Phase::Report,
        Step::Done {
            say: summary_text(&store.summary()?),
        },
    ))
}

fn summary_text(s: &Summary) -> String {
    if s.total == 0 {
        return "I didn't find any accounts linked to your email from the sources checked. Run `accountsweep list` any time to see stored results.".into();
    }
    let count = |k: &str| s.by_type.get(k).copied().unwrap_or(0);
    let mut parts = Vec::new();
    for (key, singular, plural) in [
        ("account", "account", "accounts"),
        ("subscription", "subscription", "subscriptions"),
        ("third_party_app", "connected app", "connected apps"),
        ("sign_in", "sign-in alert", "sign-in alerts"),
        (
            "newsletter",
            "newsletter or mailing list",
            "newsletters or mailing lists",
        ),
    ] {
        let n = count(key);
        if n > 0 {
            parts.push(format!("{n} {}", if n == 1 { singular } else { plural }));
        }
    }
    format!(
        "Found {} item{} linked to your email: {}. Breach checks and recommendations aren't available yet in this version. To see the full list yourself, run `accountsweep list` in a terminal.",
        s.total,
        if s.total == 1 { "" } else { "s" },
        parts.join(", ")
    )
}

/// Record an answer to a question the tool asked.
pub fn answer(store: &Store, key: &str, value: &str) -> Result<()> {
    let value = value.trim().to_ascii_lowercase();
    match key {
        CONSENT_KEY => {
            if value != "yes" && value != "no" {
                return Err(invalid(key, "yes or no"));
            }
        }
        SOURCES_KEY => {
            if store.answer(CONSENT_KEY)?.as_deref() != Some("yes") {
                return Err(Error::coded(
                    ErrorCode::ConsentRequired,
                    "ask for consent first (run `accountsweep next --agent`)",
                ));
            }
            let chosen: Vec<&str> = value
                .split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .collect();
            let allowed: Vec<&str> = SOURCE_CHOICES.iter().map(|c| c.value).collect();
            if chosen.is_empty() || chosen.iter().any(|c| !allowed.contains(c)) {
                return Err(invalid(key, &allowed.join(", ")));
            }
        }
        other => {
            return Err(Error::coded(
                ErrorCode::InvalidAnswer,
                format!("'{other}' is not a question the tool asked"),
            ));
        }
    }
    store.set_answer(key, &value)
}

fn invalid(key: &str, expected: &str) -> Error {
    Error::coded(
        ErrorCode::InvalidAnswer,
        format!("invalid answer for '{key}': expected {expected}"),
    )
}

/// Skip a source (e.g. the agent has no Gmail connector).
pub fn skip(store: &Store, source: &str) -> Result<()> {
    match source {
        "gmail" => store.set_answer(GMAIL_SKIPPED_KEY, "yes"),
        other => Err(Error::coded(
            ErrorCode::InvalidAnswer,
            format!("unknown source '{other}'. Supported: gmail"),
        )),
    }
}
