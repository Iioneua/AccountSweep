//! Workflow + ingest tests against a real (temporary) database, using the fake
//! Gmail sweep in `fixtures/sweep/gmail/`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use accountsweep_core::flow::{self, Phase, Step};
use accountsweep_core::ingest::{self, Kind};
use accountsweep_core::model::AccountType;
use accountsweep_core::output::ErrorCode;
use accountsweep_core::rules::rules;
use accountsweep_core::schemas;
use accountsweep_core::store::{self, Store};
use serde_json::Value;

/// A temporary data folder, removed on drop.
struct TempHome(PathBuf);

impl TempHome {
    fn new(name: &str) -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "accountsweep-core-{}-{}-{}",
            std::process::id(),
            name,
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        TempHome(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sweep_file(query_id: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/sweep/gmail")
        .join(format!("{query_id}.json"));
    fs::read_to_string(path).expect("sweep fixture exists")
}

fn assert_valid_instruction(instruction: &flow::Instruction) {
    let doc: Value =
        serde_json::from_str(schemas::get("next.v1").expect("schema").json).expect("schema JSON");
    let validator = jsonschema::draft202012::new(&doc).expect("compiles");
    let value = serde_json::to_value(instruction).expect("serializes");
    let errors: Vec<String> = validator
        .iter_errors(&value)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "invalid next.v1: {errors:?}\n{value}");
}

fn consented(name: &str) -> (TempHome, Store) {
    let home = TempHome::new(name);
    let store = Store::open(home.path()).expect("opens");
    flow::answer(&store, "consent", "yes").expect("consent");
    flow::answer(&store, "sources", "gmail").expect("sources");
    (home, store)
}

#[test]
fn full_basic_sweep_follows_the_state_machine() {
    let home = TempHome::new("flow");
    let store = Store::open(home.path()).expect("opens");

    let i = flow::next(&store).expect("next");
    assert_valid_instruction(&i);
    assert_eq!(i.phase, Phase::Setup);
    assert!(matches!(i.step, Step::AskUser { key: "consent", .. }));

    flow::answer(&store, "consent", "YES").expect("consent is case-insensitive");
    let i = flow::next(&store).expect("next");
    assert_valid_instruction(&i);
    assert!(matches!(i.step, Step::AskUser { key: "sources", .. }));

    flow::answer(&store, "sources", "gmail").expect("sources");
    let i = flow::next(&store).expect("next");
    assert_valid_instruction(&i);
    let Step::ConnectorRead { queries, .. } = &i.step else {
        panic!("expected connector_read, got {i:?}");
    };
    assert_eq!(queries.len(), rules().queries.len());

    for q in &rules().queries {
        ingest::ingest(&store, Kind::GmailHeaders, &sweep_file(&q.id)).expect("ingest");
        let i = flow::next(&store).expect("next");
        assert_valid_instruction(&i);
    }

    let i = flow::next(&store).expect("next");
    assert_eq!(i.phase, Phase::Report);
    assert!(matches!(i.step, Step::Done { .. }));
    assert!(i.say().contains("Found 10 items"), "summary: {}", i.say());
}

#[test]
fn grouping_and_classification_match_expectations() {
    let (_home, store) = consented("grouping");
    for q in &rules().queries {
        ingest::ingest(&store, Kind::GmailHeaders, &sweep_file(&q.id)).expect("ingest");
    }
    let accounts = store.accounts().expect("accounts");
    let find = |site: &str| {
        accounts
            .iter()
            .find(|a| a.site == site)
            .unwrap_or_else(|| panic!("missing {site}: {accounts:#?}"))
    };

    let shop = find("Example Shop");
    assert_eq!(shop.kind, AccountType::Account);
    assert_eq!(shop.domain.as_deref(), Some("shop.example"));
    assert_eq!(shop.first_seen.as_deref(), Some("2021-03-02"));
    assert_eq!(shop.confidence, 99);

    let notes = find("Example Notes");
    assert_eq!(
        notes.domain.as_deref(),
        Some("notes.example"),
        "subdomain grouped"
    );
    assert_eq!(notes.kind, AccountType::Account);

    let game = find("Example Game");
    assert_eq!(game.domain, None, "relay mail has no known domain");

    assert_eq!(find("Example Stream").kind, AccountType::Subscription);
    assert_eq!(find("Example News").kind, AccountType::Newsletter);

    let apps: Vec<_> = accounts
        .iter()
        .filter(|a| a.kind == AccountType::ThirdPartyApp)
        .collect();
    assert_eq!(apps.len(), 3, "{apps:#?}");
    assert!(
        apps.iter()
            .any(|a| a.label.as_deref() == Some("Example Notes")
                && a.provider.as_deref() == Some("google"))
    );
    assert!(
        apps.iter()
            .any(|a| a.provider.as_deref() == Some("microsoft"))
    );

    // Personal mail is never stored.
    assert!(accounts.iter().all(|a| !a.site.contains("Alice")));
    assert_eq!(accounts.len(), 10);
}

#[test]
fn reingesting_the_same_messages_does_not_duplicate() {
    let (_home, store) = consented("dedupe");
    let first = ingest::ingest(&store, Kind::GmailHeaders, &sweep_file("signups")).expect("1st");
    let second = ingest::ingest(&store, Kind::GmailHeaders, &sweep_file("signups")).expect("2nd");
    assert_eq!(first.stored, 4);
    assert_eq!(first.ignored_personal, 1);
    assert_eq!(second.stored, 0);
    assert_eq!(first.items_total, second.items_total);
}

#[test]
fn ingest_requires_consent() {
    let home = TempHome::new("noconsent");
    let store = Store::open(home.path()).expect("opens");
    let err = ingest::ingest(&store, Kind::GmailHeaders, &sweep_file("signups"))
        .expect_err("should fail");
    assert_eq!(err.code(), ErrorCode::ConsentRequired);
    assert_eq!(store.summary().expect("summary").total, 0);
}

#[test]
fn ingest_rejects_bodies_without_echoing_content() {
    let (_home, store) = consented("snippet");
    let input = r#"{"schema":"accountsweep.ingest.gmail_headers.v1","query_id":"signups","items":[
        {"message_id":"<x@shop.example>","from":"Shop <a@shop.example>","subject":"Welcome","date":"2024-01-01",
         "snippet":"SECRET-BODY-TEXT"}]}"#;
    let err = ingest::ingest(&store, Kind::GmailHeaders, input).expect_err("should fail");
    assert_eq!(err.code(), ErrorCode::SchemaViolation);
    assert!(!err.to_string().contains("SECRET-BODY-TEXT"), "{err}");
    assert_eq!(store.summary().expect("summary").total, 0);
}

#[test]
fn ingest_rejects_unknown_queries_bad_json_and_oversized_input() {
    let (_home, store) = consented("bad");
    let unknown =
        r#"{"schema":"accountsweep.ingest.gmail_headers.v1","query_id":"everything","items":[]}"#;
    assert_eq!(
        ingest::ingest(&store, Kind::GmailHeaders, unknown)
            .expect_err("should fail")
            .code(),
        ErrorCode::UnknownQuery
    );
    assert_eq!(
        ingest::ingest(&store, Kind::GmailHeaders, "{not json")
            .expect_err("should fail")
            .code(),
        ErrorCode::InvalidJson
    );
    let huge = " ".repeat(ingest::MAX_INPUT_BYTES + 1);
    assert_eq!(
        ingest::ingest(&store, Kind::GmailHeaders, &huge)
            .expect_err("should fail")
            .code(),
        ErrorCode::InputTooLarge
    );
}

#[test]
fn answers_are_validated() {
    let home = TempHome::new("answers");
    let store = Store::open(home.path()).expect("opens");
    assert_eq!(
        flow::answer(&store, "consent", "maybe")
            .expect_err("should fail")
            .code(),
        ErrorCode::InvalidAnswer
    );
    assert_eq!(
        flow::answer(&store, "sources", "gmail")
            .expect_err("should fail")
            .code(),
        ErrorCode::ConsentRequired,
        "sources can't be chosen before consent"
    );
    flow::answer(&store, "consent", "yes").expect("ok");
    assert_eq!(
        flow::answer(&store, "sources", "gmail,dropbox")
            .expect_err("should fail")
            .code(),
        ErrorCode::InvalidAnswer
    );
    assert_eq!(
        flow::answer(&store, "password", "x")
            .expect_err("should fail")
            .code(),
        ErrorCode::InvalidAnswer
    );
}

#[test]
fn declining_consent_ends_the_sweep_without_reading() {
    let home = TempHome::new("decline");
    let store = Store::open(home.path()).expect("opens");
    flow::answer(&store, "consent", "no").expect("ok");
    let i = flow::next(&store).expect("next");
    assert_valid_instruction(&i);
    assert!(matches!(i.step, Step::Done { .. }));
}

#[test]
fn skipping_gmail_finishes_discovery() {
    let (_home, store) = consented("skip");
    flow::skip(&store, "gmail").expect("skip");
    let i = flow::next(&store).expect("next");
    assert!(matches!(i.step, Step::Done { .. }));
}

#[test]
fn purge_removes_the_database() {
    let home = TempHome::new("purge");
    {
        let store = Store::open(home.path()).expect("opens");
        flow::answer(&store, "consent", "yes").expect("ok");
    }
    assert!(home.path().join(store::DB_FILE).exists());
    assert!(store::purge(home.path()).expect("purge"));
    assert!(!home.path().join(store::DB_FILE).exists());
    assert!(!store::purge(home.path()).expect("second purge is a no-op"));
}
