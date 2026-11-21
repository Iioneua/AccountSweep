//! Data model shared by storage, the CLI and agent output.

use std::collections::BTreeMap;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::rules::Category;

/// What kind of item an account entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountType {
    /// An account the user signed up for.
    Account,
    /// A paid subscription or regular purchases.
    Subscription,
    /// An app connected to a Google or Microsoft account.
    ThirdPartyApp,
    /// A sign-in or session reported by a security alert.
    SignIn,
    /// Newsletters or other mail with no sign of an account.
    Newsletter,
}

impl AccountType {
    /// All types, in display order.
    pub const ALL: [AccountType; 5] = [
        AccountType::Account,
        AccountType::Subscription,
        AccountType::ThirdPartyApp,
        AccountType::SignIn,
        AccountType::Newsletter,
    ];

    /// Stable string form.
    pub fn as_str(self) -> &'static str {
        match self {
            AccountType::Account => "account",
            AccountType::Subscription => "subscription",
            AccountType::ThirdPartyApp => "third_party_app",
            AccountType::SignIn => "sign_in",
            AccountType::Newsletter => "newsletter",
        }
    }

    /// Parse the stored string form.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == s)
    }

    /// The type implied by a set of email categories (for site items).
    pub fn from_categories<'a>(categories: impl IntoIterator<Item = &'a Category>) -> Self {
        let mut best = AccountType::Newsletter;
        for c in categories {
            match c {
                Category::Welcome | Category::PasswordReset => return AccountType::Account,
                Category::Receipt | Category::Subscription => best = AccountType::Subscription,
                _ => {}
            }
        }
        best
    }
}

/// Confidence (0–100) that a site item is a real account, from its evidence.
pub fn site_confidence<'a>(categories: impl IntoIterator<Item = &'a Category>) -> u8 {
    let mut weights: Vec<u8> = categories
        .into_iter()
        .map(|c| match c {
            Category::Welcome => 90,
            Category::PasswordReset => 85,
            Category::Receipt => 80,
            Category::Subscription => 75,
            Category::SecurityAlert => 70,
            Category::Other => 40,
            Category::Newsletter => 30,
        })
        .collect();
    weights.sort_unstable_by(|a, b| b.cmp(a));
    match weights.split_first() {
        None => 0,
        Some((top, rest)) => {
            let bonus = u8::try_from(rest.len().min(4) * 5).unwrap_or(20);
            top.saturating_add(bonus).min(99)
        }
    }
}

/// One account, subscription, connected app or sign-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Account {
    /// Stable id (`acc_` + 12 hex).
    pub id: String,
    /// Display name.
    pub site: String,
    /// Registrable domain, when known.
    pub domain: Option<String>,
    /// Item type.
    #[serde(rename = "type")]
    pub kind: AccountType,
    /// App or device name for connected apps and sign-ins.
    pub label: Option<String>,
    /// Provider (`google`, `microsoft`) for connected apps and sign-ins.
    pub provider: Option<String>,
    /// Confidence 0–100.
    pub confidence: u8,
    /// Earliest evidence date (YYYY-MM-DD).
    pub first_seen: Option<String>,
    /// Latest evidence date (YYYY-MM-DD).
    pub last_seen: Option<String>,
    /// Evidence count per category.
    pub evidence: BTreeMap<String, u32>,
    /// Workflow status.
    pub status: String,
}

/// Lowercase hex of SHA-256, truncated to `len` hex characters.
pub fn sha256_hex(input: &str, len: usize) -> String {
    let digest = Sha256::digest(input.as_bytes());
    let mut out = String::with_capacity(64);
    for byte in digest.iter() {
        out.push_str(&format!("{byte:02x}"));
    }
    out.truncate(len);
    out
}

/// Deterministic id so rescans merge instead of duplicating.
pub fn account_id(key: &str) -> String {
    format!("acc_{}", sha256_hex(key, 12))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_prefers_account_evidence() {
        use Category::*;
        assert_eq!(
            AccountType::from_categories(&[Receipt, Welcome]),
            AccountType::Account
        );
        assert_eq!(
            AccountType::from_categories(&[Newsletter, Receipt]),
            AccountType::Subscription
        );
        assert_eq!(
            AccountType::from_categories(&[Newsletter]),
            AccountType::Newsletter
        );
    }

    #[test]
    fn confidence_rewards_more_kinds_of_evidence_and_caps() {
        use Category::*;
        assert_eq!(site_confidence(&[]), 0);
        assert_eq!(site_confidence(&[Welcome]), 90);
        assert_eq!(site_confidence(&[Welcome, Receipt]), 95);
        assert_eq!(
            site_confidence(&[Welcome, Receipt, PasswordReset, Subscription]),
            99
        );
    }

    #[test]
    fn ids_are_stable_and_prefixed() {
        let a = account_id("site|shop.example");
        assert_eq!(a, account_id("site|shop.example"));
        assert_ne!(a, account_id("site|notes.example"));
        assert!(a.starts_with("acc_") && a.len() == 16);
    }
}
