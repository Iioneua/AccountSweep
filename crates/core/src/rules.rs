//! Data-driven rules embedded in the binary: the Gmail query pack, sender
//! lists and subject rules (see `crates/core/data/*.toml`).
//!
//! Contributors tune behaviour by editing those TOML files; tests validate them.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

const QUERIES_TOML: &str = include_str!("../data/queries.toml");
const SENDERS_TOML: &str = include_str!("../data/senders.toml");
const SUBJECTS_TOML: &str = include_str!("../data/subjects.toml");

/// What an email says about the sender's relationship to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Sign-up, welcome or email verification.
    Welcome,
    /// Password reset or change.
    PasswordReset,
    /// Receipt, invoice or order.
    Receipt,
    /// Subscription, renewal or trial.
    Subscription,
    /// Security alert from an account provider.
    SecurityAlert,
    /// Newsletter or digest.
    Newsletter,
    /// Nothing recognisable.
    Other,
}

impl Category {
    /// Stable string form, as stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Welcome => "welcome",
            Category::PasswordReset => "password_reset",
            Category::Receipt => "receipt",
            Category::Subscription => "subscription",
            Category::SecurityAlert => "security_alert",
            Category::Newsletter => "newsletter",
            Category::Other => "other",
        }
    }

    /// Parse the stored string form.
    pub fn parse(s: &str) -> Option<Self> {
        [
            Category::Welcome,
            Category::PasswordReset,
            Category::Receipt,
            Category::Subscription,
            Category::SecurityAlert,
            Category::Newsletter,
            Category::Other,
        ]
        .into_iter()
        .find(|c| c.as_str() == s)
    }
}

/// One Gmail search the agent runs with its connector.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    /// Stable identifier.
    pub id: String,
    /// Gmail search syntax.
    pub q: String,
    /// Fallback category when the subject matches no rule.
    pub category: Category,
    /// Result cap for this query (1–100).
    pub max_results: u32,
}

/// Sender lists.
#[derive(Debug, Clone, Default)]
pub struct Senders {
    /// Personal email providers; mail from these is ignored.
    pub freemail: BTreeSet<String>,
    /// Bulk-email relays; grouped by display name.
    pub relays: BTreeSet<String>,
    /// Sending domain → site domain.
    pub aliases: BTreeMap<String, String>,
}

/// A subject phrase list for one category.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CategoryRule {
    /// Category assigned when a phrase matches.
    pub name: Category,
    /// Lowercase phrases.
    pub contains: Vec<String>,
}

/// Kind of item a security alert reveals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    /// An app was given access to the account.
    ThirdPartyApp,
    /// A new sign-in or session.
    SignIn,
}

/// One security-alert subject rule.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlertRule {
    /// Account provider (`google`, `microsoft`).
    pub provider: String,
    /// Exact sender host the rule applies to.
    pub sender_host: String,
    /// What the alert reveals.
    #[serde(rename = "type")]
    pub kind: AlertKind,
    /// Subject ends with this; the text before it is the name.
    pub suffix: Option<String>,
    /// Subject starts with this; the text after it is the name.
    pub prefix: Option<String>,
    /// Subject contains this; no name captured.
    pub contains: Option<String>,
}

/// All embedded rules.
#[derive(Debug, Clone)]
pub struct Rules {
    /// Gmail query pack, in order.
    pub queries: Vec<Query>,
    /// Sender lists.
    pub senders: Senders,
    /// Subject categories, in priority order.
    pub categories: Vec<CategoryRule>,
    /// Security-alert rules, in priority order.
    pub alerts: Vec<AlertRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueriesFile {
    query: Vec<Query>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SendersFile {
    freemail: Vec<String>,
    relays: Vec<String>,
    aliases: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubjectsFile {
    category: Vec<CategoryRule>,
    alert: Vec<AlertRule>,
}

fn is_lower_ascii_phrase(s: &str) -> bool {
    !s.is_empty() && s.is_ascii() && s == s.to_ascii_lowercase()
}

impl Rules {
    /// Parse and validate rule files. Returns a description of the first problem.
    pub fn parse(queries: &str, senders: &str, subjects: &str) -> Result<Self, String> {
        let q: QueriesFile = toml::from_str(queries).map_err(|e| format!("queries.toml: {e}"))?;
        let s: SendersFile = toml::from_str(senders).map_err(|e| format!("senders.toml: {e}"))?;
        let sub: SubjectsFile =
            toml::from_str(subjects).map_err(|e| format!("subjects.toml: {e}"))?;

        let mut ids = BTreeSet::new();
        for query in &q.query {
            let valid_id = !query.id.is_empty()
                && query.id.len() <= 41
                && query.id.starts_with(|c: char| c.is_ascii_lowercase())
                && query
                    .id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if !valid_id {
                return Err(format!(
                    "query id '{}' must match [a-z][a-z0-9_]*",
                    query.id
                ));
            }
            if !ids.insert(query.id.clone()) {
                return Err(format!("duplicate query id '{}'", query.id));
            }
            if !(1..=100).contains(&query.max_results) {
                return Err(format!("query '{}': max_results must be 1-100", query.id));
            }
            if query.q.trim().is_empty() || query.q.len() > 500 {
                return Err(format!("query '{}': q must be 1-500 characters", query.id));
            }
        }
        if q.query.is_empty() || q.query.len() > 20 {
            return Err("queries.toml must define 1-20 queries".into());
        }

        for list in [&s.freemail, &s.relays] {
            if let Some(bad) = list.iter().find(|d| !is_lower_ascii_phrase(d)) {
                return Err(format!("senders.toml: '{bad}' must be a lowercase domain"));
            }
        }
        for rule in &sub.category {
            if let Some(bad) = rule.contains.iter().find(|p| !is_lower_ascii_phrase(p)) {
                return Err(format!(
                    "subjects.toml: phrase '{bad}' must be lowercase ASCII"
                ));
            }
        }
        for rule in &sub.alert {
            let matchers = [&rule.suffix, &rule.prefix, &rule.contains];
            let set: Vec<&String> = matchers.iter().filter_map(|m| m.as_ref()).collect();
            if set.len() != 1 {
                return Err(format!(
                    "subjects.toml: alert for {} must have exactly one of suffix/prefix/contains",
                    rule.sender_host
                ));
            }
            if !set.iter().all(|p| is_lower_ascii_phrase(p)) {
                return Err("subjects.toml: alert phrases must be lowercase ASCII".into());
            }
        }

        Ok(Rules {
            queries: q.query,
            senders: Senders {
                freemail: s.freemail.into_iter().collect(),
                relays: s.relays.into_iter().collect(),
                aliases: s.aliases,
            },
            categories: sub.category,
            alerts: sub.alert,
        })
    }

    /// Look up a query by id.
    pub fn query(&self, id: &str) -> Option<&Query> {
        self.queries.iter().find(|q| q.id == id)
    }
}

/// The rules embedded in this binary (parsed once).
pub fn rules() -> &'static Rules {
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| {
        Rules::parse(QUERIES_TOML, SENDERS_TOML, SUBJECTS_TOML)
            .expect("embedded rule files are validated by tests")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_rules_are_valid() {
        let r = Rules::parse(QUERIES_TOML, SENDERS_TOML, SUBJECTS_TOML).expect("valid rules");
        assert!(r.query("signups").is_some());
        assert!(r.senders.freemail.contains("gmail.com"));
        assert!(!r.alerts.is_empty());
    }

    #[test]
    fn category_round_trips() {
        for c in [Category::Welcome, Category::Other, Category::SecurityAlert] {
            assert_eq!(Category::parse(c.as_str()), Some(c));
        }
        assert_eq!(Category::parse("nope"), None);
    }

    #[test]
    fn rejects_alert_with_two_matchers() {
        let subjects = r#"
            category = []
            [[alert]]
            provider = "google"
            sender_host = "accounts.google.com"
            type = "sign_in"
            prefix = "a"
            contains = "b"
        "#;
        assert!(Rules::parse(QUERIES_TOML, SENDERS_TOML, subjects).is_err());
    }

    #[test]
    fn rejects_out_of_range_max_results() {
        let queries = r#"
            [[query]]
            id = "all"
            q = "in:anywhere"
            category = "welcome"
            max_results = 5000
        "#;
        assert!(Rules::parse(queries, SENDERS_TOML, SUBJECTS_TOML).is_err());
    }
}
