//! End-to-end tests of the `accountsweep` binary.
//!
//! - every `--json` / `--agent` output is one line that validates against its schema,
//! - a full Basic sweep runs in `--agent` mode within the token budget,
//! - exit codes reflect success or failure.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use accountsweep_core::output::AGENT_OUTPUT_LIMIT;
use accountsweep_core::rules::rules;
use accountsweep_core::schemas;
use serde_json::Value;

/// Total agent-facing output allowed for a full fixture sweep (SPEC §5),
/// excluding what the agent reads through its own connectors.
const TOKEN_BUDGET_BYTES: usize = 12 * 1024;

struct TempHome(PathBuf);

impl TempHome {
    fn new(name: &str) -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "accountsweep-cli-{}-{}-{}",
            std::process::id(),
            name,
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        TempHome(dir)
    }
    fn arg(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    run_with_stdin(args, None)
}

fn run_with_stdin(args: &[&str], stdin: Option<&str>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_accountsweep"))
        .args(args)
        .env_remove("ACCOUNTSWEEP_HOME")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary runs");
    {
        let mut pipe = child.stdin.take().expect("stdin");
        if let Some(input) = stdin {
            pipe.write_all(input.as_bytes()).expect("write stdin");
        }
    }
    child.wait_with_output().expect("binary finishes")
}

fn validate(schema: &str, value: &Value) {
    let doc: Value =
        serde_json::from_str(schemas::get(schema).expect("schema exists").json).expect("JSON");
    let validator = jsonschema::draft202012::new(&doc).expect("schema compiles");
    let errors: Vec<String> = validator
        .iter_errors(value)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "{schema} invalid: {errors:?}\n{value}");
}

/// Parse the single stdout line and validate it against `schema`.
fn single_line(out: &Output, schema: &str) -> Value {
    let stdout = String::from_utf8(out.stdout.clone()).expect("stdout is UTF-8");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "must print exactly one line, got: {stdout}");
    let value: Value = serde_json::from_str(lines[0]).expect("stdout is JSON");
    validate(schema, &value);
    value
}

fn sweep_file(query_id: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../core/tests/fixtures/sweep/gmail")
        .join(format!("{query_id}.json"));
    fs::read_to_string(path).expect("sweep fixture exists")
}

/// Runs an agent-mode command, checks schema + size, and adds to the budget.
struct AgentSession {
    home: TempHome,
    bytes: usize,
}

impl AgentSession {
    fn new(name: &str) -> Self {
        AgentSession {
            home: TempHome::new(name),
            bytes: 0,
        }
    }

    fn call(&mut self, args: &[&str], stdin: Option<&str>) -> Value {
        let home = self.home.arg();
        let mut full = vec!["--agent", "--home", &home];
        full.extend_from_slice(args);
        let out = run_with_stdin(&full, stdin);
        let schema = if args.first() == Some(&"next") {
            "next.v1"
        } else {
            "envelope.v1"
        };
        let value = single_line(&out, schema);
        let size = out.stdout.len();
        assert!(
            size <= AGENT_OUTPUT_LIMIT,
            "`{}` printed {size} bytes; agent outputs must stay under {AGENT_OUTPUT_LIMIT}",
            args.join(" ")
        );
        self.bytes += size;
        value
    }
}

#[test]
fn full_basic_sweep_in_agent_mode_stays_within_token_budget() {
    let mut s = AgentSession::new("sweep");

    let i = s.call(&["next"], None);
    assert_eq!(i["do"], "ask_user");
    assert_eq!(i["key"], "consent");

    let mut answer: Vec<String> = serde_json::from_value(i["answer_with"].clone()).expect("argv");
    answer.push("yes".into());
    let args: Vec<&str> = answer[1..].iter().map(String::as_str).collect();
    assert_eq!(s.call(&args, None)["ok"], true);

    let i = s.call(&["next"], None);
    assert_eq!(i["key"], "sources");
    assert_eq!(s.call(&["answer", "sources", "gmail"], None)["ok"], true);

    let i = s.call(&["next"], None);
    assert_eq!(i["do"], "connector_read");
    let ingest_with: Vec<String> = serde_json::from_value(i["ingest_with"].clone()).expect("argv");
    let ingest_args: Vec<&str> = ingest_with[1..].iter().map(String::as_str).collect();

    for q in i["queries"].as_array().expect("queries") {
        let id = q["id"].as_str().expect("id");
        let env = s.call(&ingest_args, Some(&sweep_file(id)));
        assert_eq!(env["ok"], true, "{env}");
    }

    let i = s.call(&["next"], None);
    assert_eq!(i["do"], "done");
    assert!(i["say"].as_str().expect("say").contains("Found 10 items"));

    let list = s.call(&["list"], None);
    assert_eq!(list["data"]["total"], 10);

    assert!(
        s.bytes <= TOKEN_BUDGET_BYTES,
        "agent-facing output was {} bytes; budget is {TOKEN_BUDGET_BYTES}",
        s.bytes
    );
}

#[test]
fn connector_read_lists_every_query_in_the_pack() {
    let mut s = AgentSession::new("pack");
    s.call(&["answer", "consent", "yes"], None);
    s.call(&["answer", "sources", "gmail"], None);
    let i = s.call(&["next"], None);
    let ids: Vec<&str> = i["queries"]
        .as_array()
        .expect("queries")
        .iter()
        .map(|q| q["id"].as_str().expect("id"))
        .collect();
    let expected: Vec<&str> = rules().queries.iter().map(|q| q.id.as_str()).collect();
    assert_eq!(ids, expected);
}

#[test]
fn ingest_errors_are_enveloped_and_do_not_echo_content() {
    let mut s = AgentSession::new("ingest-err");
    let env = s.call(
        &["ingest", "--kind", "gmail_headers"],
        Some(&sweep_file("signups")),
    );
    assert_eq!(env["error"]["code"], "CONSENT_REQUIRED");

    s.call(&["answer", "consent", "yes"], None);
    let bad = r#"{"schema":"accountsweep.ingest.gmail_headers.v1","query_id":"signups","items":[
        {"message_id":"<x@shop.example>","from":"a@shop.example","subject":"Hi","date":"2024-01-01","body":"PRIVATE-TEXT"}]}"#;
    let env = s.call(&["ingest", "--kind", "gmail_headers"], Some(bad));
    assert_eq!(env["error"]["code"], "SCHEMA_VIOLATION");
    assert!(!env.to_string().contains("PRIVATE-TEXT"));

    let env = s.call(&["ingest", "--kind", "passwords"], Some("{}"));
    assert_eq!(env["error"]["code"], "UNKNOWN_KIND");
}

#[test]
fn purge_needs_confirmation_then_deletes() {
    let mut s = AgentSession::new("purge");
    s.call(&["answer", "consent", "yes"], None);
    assert_eq!(s.call(&["purge"], None)["error"]["code"], "USAGE");
    assert_eq!(s.call(&["purge", "--yes"], None)["data"]["deleted"], true);
    // After purge the tool starts over and asks for consent again.
    assert_eq!(s.call(&["next"], None)["key"], "consent");
}

#[test]
fn consent_reset_forgets_the_answer() {
    let mut s = AgentSession::new("consent");
    s.call(&["answer", "consent", "yes"], None);
    assert_eq!(s.call(&["consent"], None)["data"]["consent"], "yes");
    assert_eq!(
        s.call(&["consent", "--reset"], None)["data"]["consent"],
        Value::Null
    );
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
    let env = single_line(&out, "envelope.v1");
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
    let env = single_line(&out, "envelope.v1");
    assert_eq!(env["error"]["code"], "UNKNOWN_SCHEMA");
}

#[test]
fn errors_in_human_mode_go_to_stderr_only() {
    let out = run(&["schemas", "show", "nope.v9"]);
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown schema"));
}
