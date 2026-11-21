# Decisions log

Running record of project decisions. **[delegated]** = you said "you decide" (or set a principle) and I picked the option that follows it. Superseded decisions stay listed so the history is visible.

## Guiding principles (2026-11-21)

- **P1 Code first:** anything deterministic is done by the tool, not by agent reasoning.
- **P2 Basic is simple and inside the agent's environment:** no API keys, OAuth setup or multi-step user work by default; those are Advanced, opt-in per feature.
- **P2b Security sweet spot:** protections that cost the user nothing are always on (Basic). Secure-but-complicated protections are Advanced. Simple paths where the agent reads data are used in Basic after the user consents; the user trusts their AI provider with that data, and the AccountSweep project never receives it.
- **P3 Most secure, well-known method per step:** use established standards and widely reviewed tools; no custom crypto or ad-hoc schemes (SPEC §11.1).

## Active

| ID | Decision | Why |
|---|---|---|
| D1 | v0.1 targets technical users; Windows first; English only | Interview R1/R3 |
| D2 | Gmail is the only email source in v0.1 | Interview R1 |
| D3 | **[delegated]** Export formats: Bitwarden, Chrome/Edge CSV, generic CSV; 1Password deferred | README formats |
| D10 | **[delegated]** Recipes in TOML | `serde_yaml` deprecated |
| D13 | Append-only audit log, kept until `purge` | Interview R4 |
| D14 | Results labelled `done_verified` vs `done_user_confirmed` (+ failed / handed_to_user / skipped) | Honesty |
| D18 | MIT OR Apache-2.0; GitHub Releases + `cargo install`; GitHub Actions CI | Interview R3 |
| D19 | Name stays "AccountSweep" | Interview R3 |
| D24 | After approval, the agent may click through logout/revoke in a visible browser; hands over at sign-in/2FA/CAPTCHA/payment; final delete click is the user's | Interview R5 |
| D31 | Recipes declare `agent_clicks`; sites without a recipe need the user's OK before agent clicks | ToS stance |
| D34 | **[delegated, P1+P2]** Rust tool does OAuth, scanning, import, grouping, breach, news, recommendations, reports, state tracking. Agent does conversation, browser reading/acting, connector fallback | Principles P1/P2 |
| D35 | **[delegated, P1]** Workflow is a state machine in the tool; the agent loops on `accountsweep next --agent` and handles 8 instruction types | Fewer tokens, same behaviour every run, thin skill |
| D37 | **[delegated, P2]** HIBP catalog downloaded whole and matched locally | HIBP never sees which sites you use |
| D38 | **[delegated, P1]** News via GDELT in code with outlet-tier ranking | No agent tokens for search/judgment |
| D39 | **[delegated, P1]** Recommendations from a TOML rule table with reason codes | Deterministic, contributor-editable |
| D41 | **[delegated, P1]** Agent receives one action card at a time; verification by re-reading the page and diffing in code | Minimal context per task; code-based verification |
| D42 | **[delegated]** SQLite + OS keychain for state and secrets; the skill requires the binary (no Python scripts) | One implementation of every deterministic step |
| D43 | **[delegated, P1]** CI enforces a token budget on `--agent` output for a fixture sweep | Makes P1 measurable |
| D44 | **[delegated]** v0.1 targets local agents (Claude Code); claude.ai web deferred | It can't run the local binary |
| D45 | **[delegated, P3]** OAuth per RFC 8252 + PKCE S256 + state; revoke per RFC 7009 | Standard for native apps |
| D47 | **[delegated, P3]** Audit log is a SHA-256 hash chain with `log --verify` | Tamper evidence with a well-known technique |
| D48 | **[delegated, P3]** rustls + OS trust store, host allow-list, HTTPS only | Memory-safe TLS, no custom trust |
| D49 | **[delegated, P3]** Local web review hardened per OWASP (127.0.0.1, token, Host/Origin checks, CSRF, CSP) | Prevents other local pages / DNS rebinding from driving it |
| D50 | **[delegated, P3]** Supply chain: committed lockfile, cargo deny + cargo audit, SHA-pinned Actions, GitHub artifact attestations (SLSA provenance) | OpenSSF / SLSA practices |
| D51 | **[delegated, P3]** `#![forbid(unsafe_code)]`, `secrecy`/`zeroize` for secrets in memory | Reduce memory-safety and leak risks |
| D52 | TUI review only in v0.1; hardened local web page moves to v0.2 | Interview R6: smaller attack surface, ~2 weeks saved |
| D55 | Basic/Advanced split: Basic needs no keys, OAuth or multi-step user work; Advanced = own Gmail OAuth, HIBP key, password exports, TUI | User direction 2026-11-21 |
| D56 | Basic Gmail = agent's Gmail connector running the tool's fixed, capped query pack (`connector_read`), piped to `ingest gmail_headers` | Simple, inside the agent env; tool still owns the logic |
| D57 | Basic review/approval in chat: tool-made numbered list → user reply → `approve --chat` → tool read-back → explicit yes | No terminal needed |
| D58 | The agent installs the binary itself (release download + SHA-256 / attestation check) | No setup steps for Basic users |
| D59 | Milestones deliver the Basic path end to end first (M1–M3), Advanced after (M4–M5) | Basic is the main product |
| D60 | Security sweet spot: free protections always on; complicated ones Advanced; agent-reads-data paths allowed in Basic after one recorded user consent | User direction 2026-11-21 |
| D61 | Default build uses plain SQLite with user-only permissions + disk-encryption advice; SQLCipher is an Advanced build feature (`encrypted-db`, `db encrypt`) | Removes C crypto build dependency from Basic; encryption stays available |
| D62 | Consent step at setup names what the AI provider sees and that the project receives nothing; `consent --show/--revoke` | User decides whom to trust |
| D63 | Install check: SHA-256 in Basic; build attestation verification in Advanced | Needs `gh`, so it's an extra tool |
| D64 | `MASTER_PROMPT.md` kept private (gitignored), removed from the published history | User request |
| D65 | **[delegated]** Schemas live in `crates/core/schemas/` (not repo root) and are embedded with `include_str!` | Keeps the crate self-contained and publishable for `cargo install` |
| D66 | **[delegated]** Toolchain pinned to Rust 1.99.0 in `rust-toolchain.toml`; edition 2024; MSRV 1.85 (set by clap/jsonschema) checked in CI | Reproducible CI; declared MSRV is actually tested |
| D67 | **[delegated]** Agent commands in `next.v1` are argv arrays whose first element must be `accountsweep`; never shell strings | The tool can only ask the agent to run the tool itself; no shell injection |
| D68 | **[delegated]** `cargo-deny` covers RustSec advisories (replaces a separate `cargo audit` step); license allowlist limited to licenses in use | One well-known tool; new licenses are a reviewed change |
| D69 | **[delegated]** Fixtures must use reserved example domains, enforced by a test | Keeps real personal data out of the public repo |

## Superseded

| ID | Was | Replaced by |
|---|---|---|
| D54 | Confirmed: SQLCipher with key in keychain | D61 |
| D46 | **[delegated, P3]** Local DB encrypted with SQLCipher, key in the OS keychain | D61 (SQLCipher is Advanced) |
| D36, D53 | Own Gmail OAuth as default, connector as fallback | D55, D56 (OAuth is Advanced) |
| D40 | TUI as preferred review, chat as fallback | D57 (chat is Basic), TUI is Advanced |
| D4 | API revocation + guided manual | D24 |
| D5 | Apps from alert emails only; guided revocation | D24, D41 (alerts still parsed by the tool) |
| D6 | CLI OAuth + agent stdin | D36 |
| D7 | HIBP per-domain endpoint called by the agent | D37 |
| D8 | GDELT (rev 1) → agent web search (rev 2) | D38 (GDELT back, in code) |
| D9 | Localhost web UI | D40 (optional web review) |
| D11 | Plan hash + HMAC approvals | D40, SPEC §11 |
| D12 | `act --execute` in the CLI | D24, D41 |
| D15, D16 | Guided-only recipes; no automation in v0.1 | D24 |
| D17 | Rev 1 milestone order | SPEC §13 |
| D20 | Agent runs read-only steps unattended | D35 |
| D21 | SQLite (rev 1) → JSON file (rev 2) | D42 (SQLite back) |
| D22 | Skill is the product; agent does most work via connectors | D34 (skill is the agent interface; the tool does the work) |
| D23 | Skill for Claude Code + claude.ai | D44 |
| D25 | Offline viewer-only CLI | D34 |
| D26, D27 | Python stdlib scripts in the skill | D42 |
| D28 | Agent reads connected-apps pages | Kept, now as `browser_read` instructions (D35) |
| D29 | Approval via chat / artifact / CLI file, no HMAC | D40 |
| D30 | News via agent web search | D38 |
| D32 | Skill evals as release gate | Kept (SPEC §13) |
| D33 | `sweep.json` in `~/.accountsweep/` | D42 |
