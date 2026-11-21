//! `accountsweep` command-line tool.
//!
//! M0 scope: version and schema inspection. The workflow commands described in
//! `SPEC.md` §7 arrive in later milestones.

use std::io::Write;
use std::process::ExitCode;

use accountsweep_core::output::{Envelope, ErrorCode};
use accountsweep_core::schemas;
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
    /// Print exactly one machine-readable JSON envelope (envelope.v1) on stdout.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
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
        /// Schema name, e.g. `next.v1` (see `accountsweep schemas list`).
        name: String,
    },
}

#[derive(Debug, Serialize)]
struct SchemaInfo {
    name: &'static str,
    id: &'static str,
    summary: &'static str,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => code,
        Err(e) => {
            if cli.json {
                let env = Envelope::<()>::err(
                    command_name(&cli.command),
                    ErrorCode::Internal,
                    e.to_string(),
                );
                print_json(&env);
            } else {
                eprintln!("error: {e:#}");
            }
            ExitCode::FAILURE
        }
    }
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Schemas {
            action: SchemasCmd::List,
        } => "schemas list",
        Command::Schemas {
            action: SchemasCmd::Show { .. },
        } => "schemas show",
    }
}

fn run(cli: &Cli) -> anyhow::Result<ExitCode> {
    let name = command_name(&cli.command);
    match &cli.command {
        Command::Schemas {
            action: SchemasCmd::List,
        } => {
            let list: Vec<SchemaInfo> = schemas::ALL
                .iter()
                .map(|s| SchemaInfo {
                    name: s.name,
                    id: s.id,
                    summary: s.summary,
                })
                .collect();
            if cli.json {
                print_json(&Envelope::ok(name, list));
            } else {
                let mut out = std::io::stdout().lock();
                for s in &list {
                    writeln!(out, "{:<26} {}", s.name, s.summary)?;
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Schemas {
            action: SchemasCmd::Show { name: wanted },
        } => {
            let Some(schema) = schemas::get(wanted) else {
                let known: Vec<&str> = schemas::ALL.iter().map(|s| s.name).collect();
                let message = format!("unknown schema '{wanted}'. Known: {}", known.join(", "));
                if cli.json {
                    print_json(&Envelope::<()>::err(
                        name,
                        ErrorCode::UnknownSchema,
                        message,
                    ));
                } else {
                    eprintln!("error: {message}");
                }
                return Ok(ExitCode::FAILURE);
            };
            if cli.json {
                let doc: serde_json::Value = serde_json::from_str(schema.json)?;
                print_json(&Envelope::ok(name, doc));
            } else {
                let mut out = std::io::stdout().lock();
                out.write_all(schema.json.as_bytes())?;
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn print_json<T: Serialize>(env: &Envelope<T>) {
    match env.to_json() {
        Ok(line) => println!("{line}"),
        // Serializing our own types cannot realistically fail; keep stdout parseable anyway.
        Err(e) => println!(
            r#"{{"schema":"accountsweep.envelope.v1","command":"internal","ok":false,"error":{{"code":"INTERNAL","message":"failed to serialize output: {}"}},"warnings":[]}}"#,
            e.to_string().replace('"', "'")
        ),
    }
}
