//! `ingest`: accept data the agent read through its connectors.
//!
//! Input is untrusted. It is size-limited, schema-validated (unknown fields
//! such as `body` or `snippet` are rejected), and then reduced to the minimum:
//! only the site, an email category, a date and a hash of the message id are
//! stored. Subjects and sender addresses are never stored.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::classify::{
    SenderClass, classify_sender, classify_subject, match_alert, parse_date, parse_from,
    provider_domain, registrable_domain,
};
use crate::error::{Error, Result};
use crate::flow::{self, CONSENT_KEY};
use crate::model::{AccountType, account_id, sha256_hex};
use crate::output::ErrorCode;
use crate::rules::{AlertKind, Category, rules};
use crate::schemas;
use crate::store::{NewSighting, Store};

/// Largest accepted stdin input.
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;

/// Ingest kinds the tool accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Gmail headers from the agent's connector.
    GmailHeaders,
}

impl Kind {
    /// Parse a `--kind` value.
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "gmail_headers" => Ok(Kind::GmailHeaders),
            other => Err(Error::coded(
                ErrorCode::UnknownKind,
                format!("unknown ingest kind '{other}'. Supported: gmail_headers"),
            )),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GmailHeaders {
    #[allow(dead_code)]
    schema: String,
    query_id: String,
    items: Vec<Header>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    message_id: String,
    from: String,
    subject: String,
    date: String,
}

/// What `ingest` did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IngestReport {
    /// Query the batch belonged to.
    pub query_id: String,
    /// Headers received.
    pub received: u32,
    /// New sightings stored (duplicates are skipped).
    pub stored: u32,
    /// Messages from personal addresses, ignored.
    pub ignored_personal: u32,
    /// Messages whose sender could not be parsed, ignored.
    pub ignored_unparsed: u32,
    /// Items now known in total.
    pub items_total: u32,
    /// Queries the agent still has to run.
    pub queries_left: u32,
}

fn gmail_validator() -> &'static jsonschema::Validator {
    static V: OnceLock<jsonschema::Validator> = OnceLock::new();
    V.get_or_init(|| {
        let doc: serde_json::Value = serde_json::from_str(
            schemas::get("ingest-gmail-headers.v1")
                .expect("schema is registered")
                .json,
        )
        .expect("embedded schema is valid JSON");
        jsonschema::draft202012::new(&doc).expect("embedded schema compiles")
    })
}

/// Ingest stdin text of the given kind.
pub fn ingest(store: &Store, kind: Kind, input: &str) -> Result<IngestReport> {
    if store.answer(CONSENT_KEY)?.as_deref() != Some("yes") {
        return Err(Error::coded(
            ErrorCode::ConsentRequired,
            "the user has not agreed to Basic mode yet; run `accountsweep next --agent` first",
        ));
    }
    if input.len() > MAX_INPUT_BYTES {
        return Err(Error::coded(
            ErrorCode::InputTooLarge,
            format!("input is larger than {MAX_INPUT_BYTES} bytes"),
        ));
    }
    match kind {
        Kind::GmailHeaders => ingest_gmail(store, input),
    }
}

fn ingest_gmail(store: &Store, input: &str) -> Result<IngestReport> {
    let value: serde_json::Value = serde_json::from_str(input).map_err(|e| {
        Error::coded(
            ErrorCode::InvalidJson,
            format!(
                "input is not valid JSON (line {}, column {})",
                e.line(),
                e.column()
            ),
        )
    })?;
    // Report where the input is wrong, never what it contains.
    let problems: Vec<String> = gmail_validator()
        .iter_errors(&value)
        .take(3)
        .map(|e| {
            let at = e.instance_path().to_string();
            format!(
                "at '{}' (rule {})",
                if at.is_empty() { "/" } else { &at },
                e.schema_path()
            )
        })
        .collect();
    if !problems.is_empty() {
        return Err(Error::coded(
            ErrorCode::SchemaViolation,
            format!(
                "input does not match ingest-gmail-headers.v1 (only message_id, from, subject, date are accepted): {}",
                problems.join("; ")
            ),
        ));
    }
    let batch: GmailHeaders = serde_json::from_value(value).map_err(|_| {
        Error::coded(
            ErrorCode::SchemaViolation,
            "input does not match ingest-gmail-headers.v1",
        )
    })?;

    let r = rules();
    let query = r.query(&batch.query_id).ok_or_else(|| {
        Error::coded(
            ErrorCode::UnknownQuery,
            format!(
                "'{}' is not a query from the tool's query pack",
                batch.query_id
            ),
        )
    })?;

    let mut report = IngestReport {
        query_id: batch.query_id.clone(),
        received: u32::try_from(batch.items.len()).unwrap_or(u32::MAX),
        stored: 0,
        ignored_personal: 0,
        ignored_unparsed: 0,
        items_total: 0,
        queries_left: 0,
    };

    let mut sightings = Vec::with_capacity(batch.items.len());
    for h in &batch.items {
        let Some(sender) = parse_from(&h.from) else {
            report.ignored_unparsed += 1;
            continue;
        };
        let seen_on = parse_date(&h.date).map(|d| d.to_string());
        let message_hash = sha256_hex(&h.message_id, 32);
        let sender_domain = registrable_domain(&sender.host);

        if let Some(alert) = match_alert(&sender.host, &h.subject, r) {
            let kind = match alert.kind {
                AlertKind::ThirdPartyApp => AccountType::ThirdPartyApp,
                AlertKind::SignIn => AccountType::SignIn,
            };
            let provider_name = if alert.provider == "microsoft" {
                "Microsoft"
            } else {
                "Google"
            };
            let site = match (&alert.label, kind) {
                (Some(label), AccountType::SignIn) => format!("{provider_name} sign-in: {label}"),
                (Some(label), _) => label.clone(),
                (None, AccountType::SignIn) => format!("{provider_name} sign-ins"),
                (None, _) => format!("{provider_name} connected apps (review)"),
            };
            let key = format!(
                "alert|{}|{}|{}",
                alert.provider,
                kind.as_str(),
                alert.label.as_deref().unwrap_or("").to_lowercase()
            );
            sightings.push(NewSighting {
                account_id: account_id(&key),
                site,
                domain: Some(provider_domain(&alert.provider).to_string()),
                kind,
                label: alert.label,
                provider: Some(alert.provider),
                category: Category::SecurityAlert,
                seen_on,
                sender_domain,
                message_hash,
            });
            continue;
        }

        match classify_sender(&sender, r) {
            SenderClass::Personal => report.ignored_personal += 1,
            SenderClass::Site { domain, site } => {
                let category = classify_subject(&h.subject, r).unwrap_or(match query.category {
                    Category::SecurityAlert => Category::Other,
                    c => c,
                });
                let key = match &domain {
                    Some(d) => format!("site|{d}"),
                    None => format!("site|relay:{}", site.to_lowercase()),
                };
                sightings.push(NewSighting {
                    account_id: account_id(&key),
                    site,
                    domain,
                    kind: AccountType::from_categories(&[category]),
                    label: None,
                    provider: None,
                    category,
                    seen_on,
                    sender_domain,
                    message_hash,
                });
            }
        }
    }

    report.stored = store.ingest_batch(&batch.query_id, &sightings)?;
    report.items_total = store.summary()?.total;
    report.queries_left = u32::try_from(flow::pending_queries(store)?.len()).unwrap_or(u32::MAX);
    Ok(report)
}
