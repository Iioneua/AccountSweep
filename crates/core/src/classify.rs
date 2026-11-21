//! Pure classification functions: parse a sender, decide which site it
//! belongs to, categorise a subject, recognise security alerts, parse dates.
//!
//! Nothing here touches storage or the network, so it is easy to test.

use time::format_description::well_known::{Rfc2822, Rfc3339};
use time::{Date, OffsetDateTime};

use crate::rules::{AlertKind, Category, Rules};

/// A parsed `From` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sender {
    /// Lowercase sender host (part after `@`).
    pub host: String,
    /// Display name, if any (cleaned).
    pub display_name: Option<String>,
}

/// Maximum length kept for display names and captured labels.
const MAX_NAME_LEN: usize = 60;

fn clean_name(raw: &str) -> Option<String> {
    let collapsed: String = raw
        .trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let cleaned: String = collapsed
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_LEN)
        .collect();
    let cleaned = cleaned.trim().to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

fn is_valid_host(host: &str) -> bool {
    host.len() <= 253
        && host.contains('.')
        && !host.starts_with('.')
        && !host.contains("..")
        && host
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
}

/// Parse a `From` header such as `Example Shop <no-reply@shop.example>`.
pub fn parse_from(from: &str) -> Option<Sender> {
    let (name, addr) = match from.rfind('<') {
        Some(start) => {
            let rest = &from[start + 1..];
            let end = rest.find('>')?;
            (Some(&from[..start]), &rest[..end])
        }
        None => (None, from),
    };
    let (_, host) = addr.trim().rsplit_once('@')?;
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if !is_valid_host(&host) {
        return None;
    }
    Some(Sender {
        host,
        display_name: name.and_then(clean_name),
    })
}

/// Where a sender belongs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SenderClass {
    /// Personal mail (freemail provider): ignored entirely.
    Personal,
    /// A site or service.
    Site {
        /// Registrable domain of the site; `None` for mail sent via a bulk relay.
        domain: Option<String>,
        /// Display name for the site.
        site: String,
    },
}

/// Registrable domain (eTLD+1) of a host, using the Public Suffix List.
pub fn registrable_domain(host: &str) -> String {
    psl::domain_str(host).unwrap_or(host).to_string()
}

fn is_generic_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    const GENERIC: [&str; 8] = [
        "no-reply",
        "noreply",
        "no reply",
        "do not reply",
        "donotreply",
        "notification",
        "info",
        "support",
    ];
    GENERIC.iter().any(|g| lower == *g || lower.starts_with(g))
}

fn name_from_domain(domain: &str) -> String {
    let label = domain.split('.').next().unwrap_or(domain);
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => domain.to_string(),
    }
}

/// Decide which site a sender belongs to.
pub fn classify_sender(sender: &Sender, rules: &Rules) -> SenderClass {
    let registrable = registrable_domain(&sender.host);
    if rules.senders.freemail.contains(&registrable) {
        return SenderClass::Personal;
    }
    let display = sender.display_name.clone().filter(|n| !is_generic_name(n));
    if rules.senders.relays.contains(&registrable) {
        return match display {
            Some(site) => SenderClass::Site { domain: None, site },
            None => SenderClass::Personal,
        };
    }
    let domain = rules
        .senders
        .aliases
        .get(&registrable)
        .cloned()
        .unwrap_or(registrable);
    let site = display.unwrap_or_else(|| name_from_domain(&domain));
    SenderClass::Site {
        domain: Some(domain),
        site,
    }
}

/// Categorise a subject line. Returns `None` when no rule matches.
pub fn classify_subject(subject: &str, rules: &Rules) -> Option<Category> {
    let lower = subject.to_ascii_lowercase();
    rules
        .categories
        .iter()
        .find(|rule| rule.contains.iter().any(|p| lower.contains(p.as_str())))
        .map(|rule| rule.name)
}

/// A recognised security alert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertMatch {
    /// Provider (`google`, `microsoft`).
    pub provider: String,
    /// What the alert reveals.
    pub kind: AlertKind,
    /// App or device name, when the subject contains it.
    pub label: Option<String>,
}

/// Recognise a security alert from its sender host and subject.
pub fn match_alert(sender_host: &str, subject: &str, rules: &Rules) -> Option<AlertMatch> {
    // ASCII lowercasing keeps byte offsets identical, so captures can be sliced
    // from the original subject (preserving the app name's case).
    let lower = subject.to_ascii_lowercase();
    for rule in rules.alerts.iter().filter(|r| r.sender_host == sender_host) {
        let label = if let Some(suffix) = &rule.suffix {
            if !lower.ends_with(suffix.as_str()) {
                continue;
            }
            clean_name(&subject[..subject.len() - suffix.len()])
        } else if let Some(prefix) = &rule.prefix {
            if !lower.starts_with(prefix.as_str()) {
                continue;
            }
            clean_name(&subject[prefix.len()..])
        } else if let Some(phrase) = &rule.contains {
            if !lower.contains(phrase.as_str()) {
                continue;
            }
            None
        } else {
            continue;
        };
        return Some(AlertMatch {
            provider: rule.provider.clone(),
            kind: rule.kind,
            label,
        });
    }
    None
}

/// Site domain of an alert provider.
pub fn provider_domain(provider: &str) -> &'static str {
    match provider {
        "microsoft" => "microsoft.com",
        _ => "google.com",
    }
}

/// Parse an email date in the formats connectors commonly return:
/// RFC 2822 (`Tue, 02 Mar 2021 10:00:00 +0000 (UTC)`), RFC 3339,
/// `YYYY-MM-DD`, or Unix time in seconds or milliseconds.
pub fn parse_date(raw: &str) -> Option<Date> {
    let mut s = raw.trim();
    if let Some(idx) = s.find(" (") {
        s = s[..idx].trim_end();
    }
    if let Ok(dt) = OffsetDateTime::parse(s, &Rfc3339) {
        return Some(dt.date());
    }
    if let Ok(dt) = OffsetDateTime::parse(s, &Rfc2822) {
        return Some(dt.date());
    }
    if s.len() >= 10 {
        let fmt = time::macros::format_description!("[year]-[month]-[day]");
        if let Ok(d) = Date::parse(&s[..10], &fmt) {
            return Some(d);
        }
    }
    if !s.is_empty() && s.len() <= 13 && s.chars().all(|c| c.is_ascii_digit()) {
        let n: i64 = s.parse().ok()?;
        let secs = if s.len() == 13 { n / 1000 } else { n };
        return OffsetDateTime::from_unix_timestamp(secs)
            .ok()
            .map(|d| d.date());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::rules;

    fn sender(from: &str) -> Sender {
        parse_from(from).expect("parses")
    }

    #[test]
    fn parses_common_from_formats() {
        assert_eq!(
            sender("Example Shop <no-reply@shop.example>"),
            Sender {
                host: "shop.example".into(),
                display_name: Some("Example Shop".into())
            }
        );
        assert_eq!(
            sender("\"Notes\" <HELLO@Mail.Notes.Example>").host,
            "mail.notes.example"
        );
        assert_eq!(sender("billing@shop.example").display_name, None);
        assert_eq!(parse_from("not an address"), None);
        assert_eq!(parse_from("x <bad@host>"), None);
        assert_eq!(parse_from("x <a@b..example>"), None);
    }

    #[test]
    fn groups_subdomains_and_ignores_personal_mail() {
        let r = rules();
        assert_eq!(
            classify_sender(&sender("Example Notes <a@mail.notes.example>"), r),
            SenderClass::Site {
                domain: Some("notes.example".into()),
                site: "Example Notes".into()
            }
        );
        assert_eq!(
            classify_sender(&sender("Alice <alice@gmail.com>"), r),
            SenderClass::Personal
        );
        assert_eq!(
            classify_sender(&sender("no-reply <a@facebookmail.com>"), r),
            SenderClass::Site {
                domain: Some("facebook.com".into()),
                site: "Facebook".into()
            }
        );
    }

    #[test]
    fn relay_mail_is_grouped_by_display_name() {
        let r = rules();
        assert_eq!(
            classify_sender(&sender("Example Game <bounce@em123.sendgrid.net>"), r),
            SenderClass::Site {
                domain: None,
                site: "Example Game".into()
            }
        );
        assert_eq!(
            classify_sender(&sender("<bounce@em123.sendgrid.net>"), r),
            SenderClass::Personal
        );
    }

    #[test]
    fn categorises_subjects() {
        let r = rules();
        assert_eq!(
            classify_subject("Welcome to Example Shop!", r),
            Some(Category::Welcome)
        );
        assert_eq!(
            classify_subject("Reset your password", r),
            Some(Category::PasswordReset)
        );
        assert_eq!(
            classify_subject("Your receipt #123", r),
            Some(Category::Receipt)
        );
        assert_eq!(classify_subject("Lunch tomorrow?", r), None);
    }

    #[test]
    fn recognises_google_alerts_and_keeps_app_name_case() {
        let r = rules();
        let m = match_alert(
            "accounts.google.com",
            "Example Notes was granted access to your Google Account",
            r,
        )
        .expect("matches");
        assert_eq!(m.kind, AlertKind::ThirdPartyApp);
        assert_eq!(m.label.as_deref(), Some("Example Notes"));

        let m = match_alert("accounts.google.com", "New sign-in on Windows", r).expect("matches");
        assert_eq!(m.kind, AlertKind::SignIn);
        assert_eq!(m.label.as_deref(), Some("Windows"));

        let m = match_alert("accounts.google.com", "Security alert", r).expect("matches");
        assert_eq!(m.label, None);

        assert_eq!(match_alert("shop.example", "Security alert", r), None);
    }

    #[test]
    fn parses_dates_from_several_formats() {
        let d = |y, m, day| {
            Date::from_calendar_date(y, time::Month::try_from(m).expect("month"), day)
                .expect("date")
        };
        assert_eq!(
            parse_date("Tue, 02 Mar 2021 10:00:00 +0000 (UTC)"),
            Some(d(2021, 3, 2))
        );
        assert_eq!(parse_date("2024-08-14T09:30:00Z"), Some(d(2024, 8, 14)));
        assert_eq!(parse_date("2024-08-14"), Some(d(2024, 8, 14)));
        assert_eq!(parse_date("1723627800000"), Some(d(2024, 8, 14)));
        assert_eq!(parse_date("yesterday"), None);
    }
}
