//! Contract tests for the embedded JSON Schemas.
//!
//! - every schema is a valid draft 2020-12 schema with the expected `$id`,
//! - every file in `fixtures/valid` passes its schema,
//! - every file in `fixtures/invalid` is rejected by its schema,
//! - enums shared between schemas stay in sync,
//! - fixtures contain only reserved example domains (no real personal data).
//!
//! Fixture files are named `<schema-stem>.<case>.(json|toml)`, where the stem
//! maps to the schema `<schema-stem>.v1`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use accountsweep_core::schemas;
use serde_json::Value;

fn fixtures_dir(kind: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(kind)
}

fn fixture_files(kind: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(fixtures_dir(kind))
        .expect("fixtures dir exists")
        .map(|e| e.expect("readable entry").path())
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no fixtures in {kind}");
    files
}

/// Parse a fixture (JSON, or TOML for recipes) into a JSON value.
fn load_fixture(path: &Path) -> Value {
    let text = fs::read_to_string(path).expect("fixture is UTF-8 text");
    match path.extension().and_then(|e| e.to_str()) {
        Some("json") => serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display())),
        Some("toml") => {
            let t: toml::Value = toml::from_str(&text)
                .unwrap_or_else(|e| panic!("{} is not valid TOML: {e}", path.display()));
            serde_json::to_value(t).expect("TOML converts to JSON")
        }
        other => panic!(
            "unexpected fixture extension {other:?} for {}",
            path.display()
        ),
    }
}

fn schema_for_fixture(path: &Path) -> &'static schemas::Schema {
    let file = path
        .file_name()
        .and_then(|f| f.to_str())
        .expect("file name");
    let stem = file.split('.').next().expect("has a stem");
    let name = format!("{stem}.v1");
    schemas::get(&name).unwrap_or_else(|| panic!("{file}: no schema named {name}"))
}

fn schema_value(s: &schemas::Schema) -> Value {
    serde_json::from_str(s.json).unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", s.name))
}

fn validator(s: &schemas::Schema) -> jsonschema::Validator {
    jsonschema::draft202012::new(&schema_value(s))
        .unwrap_or_else(|e| panic!("{} does not compile: {e}", s.name))
}

#[test]
fn every_schema_is_valid_draft_2020_12_with_matching_id() {
    for s in schemas::ALL {
        let v = schema_value(s);
        assert_eq!(
            v["$schema"], "https://json-schema.org/draft/2020-12/schema",
            "{} must declare draft 2020-12",
            s.name
        );
        assert_eq!(
            v["$id"], s.id,
            "{}: $id does not match the registry",
            s.name
        );
        if let Err(e) = jsonschema::meta::validate(&v) {
            panic!("{} fails the meta-schema: {e}", s.name);
        }
        let _ = validator(s);
    }
}

#[test]
fn valid_fixtures_pass() {
    for path in fixture_files("valid") {
        let s = schema_for_fixture(&path);
        let instance = load_fixture(&path);
        let errors: Vec<String> = validator(s)
            .iter_errors(&instance)
            .map(|e| format!("{} at {}", e, e.instance_path()))
            .collect();
        assert!(
            errors.is_empty(),
            "{} should pass {} but got:\n  {}",
            path.display(),
            s.name,
            errors.join("\n  ")
        );
    }
}

#[test]
fn invalid_fixtures_are_rejected() {
    for path in fixture_files("invalid") {
        let s = schema_for_fixture(&path);
        let instance = load_fixture(&path);
        assert!(
            !validator(s).is_valid(&instance),
            "{} should be rejected by {}",
            path.display(),
            s.name
        );
    }
}

#[test]
fn every_schema_has_valid_and_invalid_fixtures() {
    for kind in ["valid", "invalid"] {
        let covered: BTreeSet<&str> = fixture_files(kind)
            .iter()
            .map(|p| schema_for_fixture(p).name)
            .collect();
        for s in schemas::ALL {
            assert!(covered.contains(s.name), "{} has no {kind} fixture", s.name);
        }
    }
}

fn pointer(schema: &str, ptr: &str) -> Value {
    let s = schemas::get(schema).expect("schema exists");
    schema_value(s)
        .pointer(ptr)
        .unwrap_or_else(|| panic!("{schema} has no {ptr}"))
        .clone()
}

#[test]
fn shared_enums_stay_in_sync() {
    assert_eq!(
        pointer("next.v1", "/$defs/browser_read/properties/kind"),
        pointer("ingest-page-items.v1", "/$defs/kind"),
        "browser_read kinds must match ingest-page-items kinds"
    );
    assert_eq!(
        pointer("next.v1", "/$defs/browser_act/properties/stop_if"),
        pointer("recipe.v1", "/$defs/stop_if"),
        "browser_act stop_if must match recipe stop_if"
    );
}

/// Domains fixtures may use: RFC 2606 / RFC 6761 reserved names, plus a few
/// public, non-personal service hosts that appear in instructions.
fn is_allowed_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    const RESERVED_SUFFIXES: [&str; 4] = [".example", ".test", ".invalid", ".localhost"];
    const RESERVED_DOMAINS: [&str; 3] = ["example.com", "example.org", "example.net"];
    const PUBLIC_SERVICE_HOSTS: [&str; 3] = [
        "myaccount.google.com",
        "accounts.google.com",
        "accountprotection.microsoft.com",
    ];
    RESERVED_SUFFIXES.iter().any(|s| host.ends_with(s))
        || RESERVED_DOMAINS
            .iter()
            .any(|d| host == *d || host.ends_with(&format!(".{d}")))
        || PUBLIC_SERVICE_HOSTS.contains(&host.as_str())
}

/// Every `(local part, host)` mentioned as an email address, plus every URL host
/// (with no local part).
fn hosts_in(text: &str) -> Vec<(Option<String>, String)> {
    let mut hosts = Vec::new();
    let is_host_char = |c: char| c.is_ascii_alphanumeric() || c == '.' || c == '-';
    let is_local_char = |c: char| c.is_ascii_alphanumeric() || "._%+-".contains(c);
    for (i, _) in text.match_indices('@') {
        let host: String = text[i + 1..]
            .chars()
            .take_while(|c| is_host_char(*c))
            .collect();
        let local: String = text[..i]
            .chars()
            .rev()
            .take_while(|c| is_local_char(*c))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if host.contains('.') {
            hosts.push((Some(local), host));
        }
    }
    for (i, _) in text.match_indices("://") {
        let host: String = text[i + 3..]
            .chars()
            .take_while(|c| is_host_char(*c))
            .collect();
        if !host.is_empty() {
            hosts.push((None, host));
        }
    }
    hosts
}

/// Real service domains fixtures may use to exercise specific rules:
/// bulk-email relays (any subdomain) and freemail providers (only with the
/// obviously fake local parts below, so no real address can slip in).
fn is_allowed_test_address(local: Option<&str>, host: &str) -> bool {
    const RELAYS: [&str; 1] = ["sendgrid.net"];
    const FREEMAIL: [&str; 1] = ["gmail.com"];
    const FAKE_LOCAL_PARTS: [&str; 2] = ["alice", "bob"];
    RELAYS.iter().any(|r| host.ends_with(&format!(".{r}")))
        || (FREEMAIL.contains(&host) && local.is_some_and(|l| FAKE_LOCAL_PARTS.contains(&l)))
}

fn all_fixture_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("readable dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            all_fixture_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn fixtures_use_only_reserved_example_domains() {
    let mut files = Vec::new();
    all_fixture_files(&fixtures_dir(""), &mut files);
    assert!(files.len() > 30, "expected to scan every fixture");
    for path in files {
        let text = fs::read_to_string(&path).expect("readable");
        for (local, host) in hosts_in(&text) {
            assert!(
                is_allowed_host(&host) || is_allowed_test_address(local.as_deref(), &host),
                "{} mentions '{host}'. Fixtures must use reserved example domains (e.g. shop.example, example.com).",
                path.display()
            );
        }
    }
}
