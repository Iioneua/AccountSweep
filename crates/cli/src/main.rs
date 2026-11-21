//! `accountsweep` command-line tool.
//!
//! Three output modes:
//! - human (default): readable text,
//! - `--json`: one `envelope.v1` line with full data,
//! - `--agent`: one compact line, capped at 2 KB (`next --agent` prints a `next.v1` instruction).

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use accountsweep_core::flow::{self, Instruction, Step};
use accountsweep_core::ingest::{self, Kind};
use accountsweep_core::model::Account;
use accountsweep_core::output::{AGENT_OUTPUT_LIMIT, Envelope, ErrorCode};
use accountsweep_core::store::{self, Store};
use accountsweep_core::{Error, schemas};
use clap::{Parser, Subcommand};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "accountsweep",
    version,
    about = "Find the accounts tied to your email, check them for breaches, and act only on what you approve.",
    long_about = None
)]
struct Cli {
    /// Print one machine-readable JSON envelope (envelope.v1) on stdout.
    #[arg(long, global = true)]
    json: bool,

    /// Compact JSON for AI agents (size-capped). `next --agent` prints a next.v1 instruction.
    #[arg(long, global = true, conflicts_with = "json")]
    agent: bool,

    /// Data folder (default: your local app-data folder; or set ACCOUNTSWEEP_HOME).
    #[arg(long, global = true, value_name = "DIR")]
    home: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show what to do next (the workflow). Agents: `next --agent`.
    Next,
    /// Record the user's answer to a question the tool asked.
    Answer {
        /// Question key, e.g. `consent`.
        key: String,
        /// The answer, e.g. `yes`, or `gmail` for sources.
        value: String,
    },
    /// Skip a source the agent can't read (e.g. no Gmail connector).
    Skip {
        /// Source to skip: `gmail`.
        source: String,
    },
    /// Read data the agent collected (JSON on stdin).
    Ingest {
        /// What the input is: `gmail_headers`.
        #[arg(long)]
        kind: String,
    },
    /// List what was found.
    List {
        /// Show at most this many items (agent mode defaults to 20).
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Show one item in detail.
    Show {
        /// Item id (from `list`).
        id: String,
    },
    /// Show your Basic-mode consent, or reset it.
    Consent {
        /// Forget your answer; the tool will ask again before anything is read.
        #[arg(long)]
        reset: bool,
    },
    /// Delete all local AccountSweep data.
    Purge {
        /// Confirm deletion.
        #[arg(long)]
        yes: bool,
    },
    /// Inspect the JSON Schemas that define the tool ↔ agent contract.
    Schemas {
        #[command(subcommand)]
        action: SchemasCmd,
    },
}

#[derive(Debug, Subcommand)]
enum SchemasCmd {
    /// List the embedded schemas.
    List,
    /// Print one schema document.
    Show {
        /// Schema name, e.g. `next.v1`.
        name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Human,
    Json,
    Agent,
}

impl Command {
    fn name(&self) -> &'static str {
        match self {
            Command::Next => "next",
            Command::Answer { .. } => "answer",
            Command::Skip { .. } => "skip",
            Command::Ingest { .. } => "ingest",
            Command::List { .. } => "list",
            Command::Show { .. } => "show",
            Command::Consent { .. } => "consent",
            Command::Purge { .. } => "purge",
            Command::Schemas {
                action: SchemasCmd::List,
            } => "schemas list",
            Command::Schemas {
                action: SchemasCmd::Show { .. },
            } => "schemas show",
        }
    }
}

/// What a command produced: human text plus machine data.
struct Output {
    human: String,
    json: serde_json::Value,
    agent: serde_json::Value,
}

impl Output {
    fn same<T: Serialize>(human: impl Into<String>, data: &T) -> Result<Self, Error> {
        let value = to_value(data)?;
        Ok(Output {
            human: human.into(),
            json: value.clone(),
            agent: value,
        })
    }
}

fn to_value<T: Serialize>(data: &T) -> Result<serde_json::Value, Error> {
    serde_json::to_value(data).map_err(|e| Error::coded(ErrorCode::Internal, e.to_string()))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let mode = if cli.agent {
        Mode::Agent
    } else if cli.json {
        Mode::Json
    } else {
        Mode::Human
    };
    let name = cli.command.name();

    // `next` in machine modes prints the instruction itself, not an envelope.
    if matches!(cli.command, Command::Next) && mode != Mode::Human {
        return match open_store(&cli).and_then(|s| flow::next(&s)) {
            Ok(instruction) => {
                print_line(&serde_json::to_string(&instruction).unwrap_or_default());
                ExitCode::SUCCESS
            }
            Err(e) => fail(mode, name, &e),
        };
    }

    match run(&cli, mode) {
        Ok(out) => {
            match mode {
                Mode::Human => {
                    let mut stdout = std::io::stdout().lock();
                    let _ = stdout.write_all(out.human.as_bytes());
                    if !out.human.ends_with('\n') {
                        let _ = stdout.write_all(b"\n");
                    }
                }
                Mode::Json => print_envelope(&Envelope::ok(name, out.json)),
                Mode::Agent => print_envelope(&Envelope::ok(name, out.agent)),
            }
            ExitCode::SUCCESS
        }
        Err(e) => fail(mode, name, &e),
    }
}

fn fail(mode: Mode, name: &str, e: &Error) -> ExitCode {
    match mode {
        Mode::Human => eprintln!("error: {e}"),
        Mode::Json | Mode::Agent => {
            print_envelope(&Envelope::<()>::err(name, e.code(), e.to_string()))
        }
    }
    ExitCode::FAILURE
}

fn print_line(line: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{line}");
}

fn print_envelope<T: Serialize>(env: &Envelope<T>) {
    match env.to_json() {
        Ok(line) => print_line(&line),
        Err(e) => print_line(&format!(
            r#"{{"schema":"accountsweep.envelope.v1","command":"internal","ok":false,"error":{{"code":"INTERNAL","message":"failed to serialize output: {}"}},"warnings":[]}}"#,
            e.to_string().replace('"', "'")
        )),
    }
}

fn home(cli: &Cli) -> Result<PathBuf, Error> {
    cli.home
        .clone()
        .or_else(|| std::env::var_os("ACCOUNTSWEEP_HOME").map(PathBuf::from))
        .or_else(store::default_home)
        .ok_or_else(|| {
            Error::coded(
                ErrorCode::Internal,
                "could not find a data folder; pass --home <DIR>",
            )
        })
}

fn open_store(cli: &Cli) -> Result<Store, Error> {
    Store::open(&home(cli)?)
}

#[derive(Serialize)]
struct Recorded<'a> {
    recorded: &'a str,
    next: &'a str,
}

#[derive(Serialize)]
struct CompactItem<'a> {
    id: &'a str,
    site: &'a str,
    #[serde(rename = "type")]
    kind: &'a str,
    conf: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_seen: Option<&'a str>,
}

#[derive(Serialize)]
struct CompactList<'a> {
    total: usize,
    shown: usize,
    more: usize,
    items: Vec<CompactItem<'a>>,
}

fn run(cli: &Cli, mode: Mode) -> Result<Output, Error> {
    match &cli.command {
        Command::Next => {
            let store = open_store(cli)?;
            let i = flow::next(&store)?;
            Ok(Output {
                human: human_instruction(&i),
                json: to_value(&i)?,
                agent: to_value(&i)?,
            })
        }
        Command::Answer { key, value } => {
            let store = open_store(cli)?;
            flow::answer(&store, key, value)?;
            Output::same(
                format!("Recorded '{key}'. Run `accountsweep next` to continue."),
                &Recorded {
                    recorded: key,
                    next: "accountsweep next --agent",
                },
            )
        }
        Command::Skip { source } => {
            let store = open_store(cli)?;
            flow::skip(&store, source)?;
            Output::same(
                format!("Skipped {source}. Run `accountsweep next` to continue."),
                &Recorded {
                    recorded: "skip",
                    next: "accountsweep next --agent",
                },
            )
        }
        Command::Ingest { kind } => {
            let kind = Kind::parse(kind)?;
            let store = open_store(cli)?;
            let input = read_stdin_limited()?;
            let report = ingest::ingest(&store, kind, &input)?;
            Output::same(
                format!(
                    "Stored {} new of {} received ({} personal, {} unreadable ignored). {} items in total; {} searches left.",
                    report.stored,
                    report.received,
                    report.ignored_personal,
                    report.ignored_unparsed,
                    report.items_total,
                    report.queries_left
                ),
                &report,
            )
        }
        Command::List { limit } => {
            let store = open_store(cli)?;
            let accounts = store.accounts()?;
            list_output(&accounts, *limit, mode)
        }
        Command::Show { id } => {
            let store = open_store(cli)?;
            let account = store.account(id)?.ok_or_else(|| {
                Error::coded(ErrorCode::NotFound, format!("no item with id '{id}'"))
            })?;
            Output::same(human_account(&account), &account)
        }
        Command::Consent { reset } => {
            let store = open_store(cli)?;
            if *reset {
                store.remove_answer(flow::CONSENT_KEY)?;
            }
            let value = store.answer(flow::CONSENT_KEY)?;
            let at = store.answered_at(flow::CONSENT_KEY)?;
            #[derive(Serialize)]
            struct ConsentStatus<'a> {
                consent: Option<&'a str>,
                answered_at: Option<&'a str>,
                text: &'a str,
            }
            let status = ConsentStatus {
                consent: value.as_deref(),
                answered_at: at.as_deref(),
                text: flow::CONSENT_TEXT,
            };
            let human = match (value.as_deref(), at.as_deref()) {
                (Some(v), Some(t)) => {
                    format!("Consent: {v} (recorded {t})\n\n{}", flow::CONSENT_TEXT)
                }
                _ => format!(
                    "Consent: not given. The tool will ask before anything is read.\n\n{}",
                    flow::CONSENT_TEXT
                ),
            };
            Output::same(human, &status)
        }
        Command::Purge { yes } => {
            if !yes {
                return Err(Error::coded(
                    ErrorCode::Usage,
                    "this deletes all local AccountSweep data; run `accountsweep purge --yes` to confirm",
                ));
            }
            let deleted = store::purge(&home(cli)?)?;
            #[derive(Serialize)]
            struct Purged {
                deleted: bool,
            }
            Output::same(
                if deleted {
                    "All local AccountSweep data was deleted."
                } else {
                    "There was no local AccountSweep data to delete."
                },
                &Purged { deleted },
            )
        }
        Command::Schemas {
            action: SchemasCmd::List,
        } => {
            #[derive(Serialize)]
            struct SchemaInfo {
                name: &'static str,
                id: &'static str,
                summary: &'static str,
            }
            let list: Vec<SchemaInfo> = schemas::ALL
                .iter()
                .map(|s| SchemaInfo {
                    name: s.name,
                    id: s.id,
                    summary: s.summary,
                })
                .collect();
            let human = list
                .iter()
                .map(|s| format!("{:<26} {}", s.name, s.summary))
                .collect::<Vec<_>>()
                .join("\n");
            Output::same(human, &list)
        }
        Command::Schemas {
            action: SchemasCmd::Show { name },
        } => {
            let schema = schemas::get(name).ok_or_else(|| {
                let known: Vec<&str> = schemas::ALL.iter().map(|s| s.name).collect();
                Error::coded(
                    ErrorCode::UnknownSchema,
                    format!("unknown schema '{name}'. Known: {}", known.join(", ")),
                )
            })?;
            let doc: serde_json::Value = serde_json::from_str(schema.json)
                .map_err(|e| Error::coded(ErrorCode::Internal, e.to_string()))?;
            Output::same(schema.json, &doc)
        }
    }
}

fn read_stdin_limited() -> Result<String, Error> {
    let mut buf = Vec::new();
    std::io::stdin()
        .take(u64::try_from(ingest::MAX_INPUT_BYTES + 1).unwrap_or(u64::MAX))
        .read_to_end(&mut buf)?;
    if buf.len() > ingest::MAX_INPUT_BYTES {
        return Err(Error::coded(
            ErrorCode::InputTooLarge,
            format!("input is larger than {} bytes", ingest::MAX_INPUT_BYTES),
        ));
    }
    String::from_utf8(buf)
        .map_err(|_| Error::coded(ErrorCode::InvalidJson, "input is not valid UTF-8"))
}

fn list_output(accounts: &[Account], limit: Option<usize>, mode: Mode) -> Result<Output, Error> {
    let total = accounts.len();
    let mut shown = limit
        .unwrap_or(if mode == Mode::Agent { 20 } else { total })
        .min(total);
    // Agent output must stay under the size cap: shrink until it fits.
    let agent = loop {
        let items = accounts[..shown]
            .iter()
            .map(|a| CompactItem {
                id: &a.id,
                site: &a.site,
                kind: a.kind.as_str(),
                conf: a.confidence,
                last_seen: a.last_seen.as_deref(),
            })
            .collect();
        let value = to_value(&CompactList {
            total,
            shown,
            more: total - shown,
            items,
        })?;
        let size = serde_json::to_string(&value).map(|s| s.len()).unwrap_or(0);
        if mode != Mode::Agent || size + 200 <= AGENT_OUTPUT_LIMIT || shown == 0 {
            break value;
        }
        shown -= 1;
    };

    let full: Vec<&Account> = accounts.iter().take(limit.unwrap_or(total)).collect();
    let mut human = String::new();
    if total == 0 {
        human.push_str("Nothing found yet. Run `accountsweep next` to start.");
    } else {
        human.push_str(&format!(
            "{:<17} {:<32} {:<16} {:>4}  {:<10}  {:<10}\n",
            "ID", "SITE", "TYPE", "CONF", "FIRST", "LAST"
        ));
        for a in &full {
            let site: String = a.site.chars().take(32).collect();
            human.push_str(&format!(
                "{:<17} {:<32} {:<16} {:>4}  {:<10}  {:<10}\n",
                a.id,
                site,
                a.kind.as_str(),
                a.confidence,
                a.first_seen.as_deref().unwrap_or("-"),
                a.last_seen.as_deref().unwrap_or("-"),
            ));
        }
        if full.len() < total {
            human.push_str(&format!("… {} more (use --limit)\n", total - full.len()));
        }
    }
    Ok(Output {
        human,
        json: to_value(&full)?,
        agent,
    })
}

fn human_account(a: &Account) -> String {
    let mut s = format!("{}  ({})\n", a.site, a.id);
    let mut line = |k: &str, v: Option<&str>| {
        if let Some(v) = v {
            s.push_str(&format!("  {k:<11} {v}\n"));
        }
    };
    line("type", Some(a.kind.as_str()));
    line("domain", a.domain.as_deref());
    line("label", a.label.as_deref());
    line("provider", a.provider.as_deref());
    line("confidence", Some(&a.confidence.to_string()));
    line("first seen", a.first_seen.as_deref());
    line("last seen", a.last_seen.as_deref());
    line("status", Some(&a.status));
    let evidence = a
        .evidence
        .iter()
        .map(|(k, v)| format!("{k} ×{v}"))
        .collect::<Vec<_>>()
        .join(", ");
    line("evidence", Some(&evidence));
    s
}

fn human_instruction(i: &Instruction) -> String {
    match &i.step {
        Step::AskUser {
            key,
            input,
            options,
            say,
            ..
        } => {
            let hint = match input {
                flow::InputKind::YesNo => "yes|no".to_string(),
                _ => options
                    .iter()
                    .map(|o| o.value)
                    .collect::<Vec<_>>()
                    .join(","),
            };
            format!("{say}\n\nAnswer with: accountsweep answer {key} <{hint}>")
        }
        Step::ConnectorRead { queries, .. } => format!(
            "Next step: your AI agent searches your Gmail with its connector ({} searches) and passes the results to `accountsweep ingest`.\nAsk your agent to continue the sweep, or run `accountsweep skip gmail` to skip Gmail.",
            queries.len()
        ),
        Step::Done { say } => say.clone(),
    }
}
