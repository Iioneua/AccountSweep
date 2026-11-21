# Contributing to AccountSweep

Thanks for helping. Please read this first; it's short.

## Ground rules

- **Never include real personal data** (email addresses, account lists, tokens, exports, screenshots of account pages) in issues, pull requests, fixtures or logs. Use reserved example domains: `example.com`, `example.org`, `*.example`, `*.test`. A test enforces this for fixtures.
- **Open an issue before large changes**, so we can agree on the approach first.
- **Security problems go through private reporting**, not issues. See [SECURITY.md](SECURITY.md).
- Follow the design rules in [SPEC.md §0](SPEC.md): code first, Basic stays simple, Advanced holds anything that needs setup, and well-known security methods only.

## Development setup

You need Rust via [rustup](https://rustup.rs). The repository pins the toolchain in `rust-toolchain.toml`, and rustup installs it automatically.

```bash
cargo fmt --all                                         # format
cargo clippy --workspace --all-targets -- -D warnings   # lint (CI fails on any warning)
cargo test --workspace                                  # tests
cargo deny check                                        # advisories, licenses, sources (cargo install cargo-deny)
```

CI runs all of these on Windows, macOS and Linux, plus a minimum-Rust-version check (1.89) and a secret scan.

## Project layout

```
crates/core/          library: data model, storage, workflow, agent contract
  data/               rules as TOML: Gmail query pack, sender lists, subject rules
  schemas/            JSON Schemas (the tool ↔ agent contract)
  tests/fixtures/     valid/ and invalid/ schema examples, plus a fake Gmail sweep
crates/cli/           the `accountsweep` binary
skill/accountsweep/   the agent skill (SKILL.md + references)
```

`recipes/` and `docs/` arrive with later milestones.

## Tuning rules

Grouping and classification are data, not code. To add a subject phrase, a bulk-email relay or a Gmail search, edit the TOML in `crates/core/data/` and run `cargo test -p accountsweep-core`. Tests validate the files and replay the fake sweep.

## Changing a schema

Schemas are a contract with agents, so treat them carefully:
- Add at least one `valid/` and one `invalid/` fixture for any new rule (`<schema-stem>.<case>.json`).
- Breaking changes need a new version (`next.v2`), not an edit of `v1`.
- Run `cargo test -p accountsweep-core`.

## Commits and pull requests

- Small, focused commits with clear messages (e.g. `core: validate ingest size limits`).
- Explain *why* in the PR description, and note any security or privacy impact.
- New dependencies need a reason in the PR: we keep the dependency set small, and `cargo deny` must pass.

## Licensing

By contributing, you agree that your contributions are dual-licensed under MIT OR Apache-2.0, like the project, without additional terms.
