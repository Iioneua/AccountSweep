//! End-to-end tests of the `accountsweep` binary.
//!
//! Every `--json` output must be a single line that validates against the
//! `envelope.v1` schema; exit codes must reflect success or failure.

use std::process::{Command, Output};

use accountsweep_core::schemas;
use serde_json::Value;

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_accountsweep"))
        .args(args)
        .output()
        .expect("binary runs")
}

fn envelope(out: &Output) -> Value {
    let stdout = String::from_utf8(out.stdout.clone()).expect("stdout is UTF-8");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "--json must print exactly one line, got: {stdout}"
    );
    let value: Value = serde_json::from_str(lines[0]).expect("stdout is JSON");

    let schema_doc: Value =
        serde_json::from_str(schemas::get("envelope.v1").expect("envelope schema").json)
            .expect("schema is JSON");
    let validator = jsonschema::draft202012::new(&schema_doc).expect("schema compiles");
    let errors: Vec<String> = validator
        .iter_errors(&value)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "envelope invalid: {errors:?}\n{value}");
    value
}

#[test]
fn version_flag_prints_crate_version() {
    let out = run(&["--version"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")), "got: {stdout}");
}

#[test]
fn schemas_list_json_is_a_valid_envelope_listing_every_schema() {
    let out = run(&["schemas", "list", "--json"]);
    assert!(out.status.success());
    let env = envelope(&out);
    assert_eq!(env["ok"], true);
    let names: Vec<&str> = env["data"]
        .as_array()
        .expect("data is a list")
        .iter()
        .map(|s| s["name"].as_str().expect("name"))
        .collect();
    let expected: Vec<&str> = schemas::ALL.iter().map(|s| s.name).collect();
    assert_eq!(names, expected);
}

#[test]
fn schemas_show_prints_the_schema_document() {
    let out = run(&["schemas", "show", "next.v1"]);
    assert!(out.status.success());
    let doc: Value = serde_json::from_slice(&out.stdout).expect("schema JSON");
    assert_eq!(doc["$id"], "urn:accountsweep:schema:next:v1");
}

#[test]
fn unknown_schema_fails_with_stable_error_code() {
    let out = run(&["schemas", "show", "nope.v9", "--json"]);
    assert!(!out.status.success());
    let env = envelope(&out);
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "UNKNOWN_SCHEMA");
}

#[test]
fn unknown_schema_without_json_writes_to_stderr_only() {
    let out = run(&["schemas", "show", "nope.v9"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown schema"));
}
