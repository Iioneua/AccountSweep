# Security policy

AccountSweep touches sensitive things: your inbox metadata, your account list, and actions on your accounts. Security reports are very welcome.

## Reporting a vulnerability

**Please don't open a public issue for security problems.** Use GitHub's private reporting instead:
**Security** tab → **Report a vulnerability** (on this repository).

Please include:
- what you found and where (file, command, recipe, schema),
- how to reproduce it, using fake data only (`example.com`, `*.example`),
- what an attacker could do with it.

**Never include real email addresses, tokens, passwords or exports** in a report.

This is a small volunteer project. We aim to acknowledge reports within 7 days and to agree on a fix and disclosure date with you.

## Supported versions

AccountSweep is pre-alpha. Only the latest commit on `main` (and, once released, the latest release) receives fixes.

## Scope

In scope:
- the `accountsweep` binary and its crates,
- the agent skill (`skill/`) and the instructions it gives an agent,
- site recipes and JSON Schemas,
- CI and release workflows (supply chain).

Out of scope: vulnerabilities in third-party services (Gmail, HIBP, websites the recipes point to) or in the AI agent itself. Report those to their owners.

## Known limits (by design, documented honestly)

- **Approval relies on the agent following the skill.** The tool records approvals and reads them back before anything happens, but an agent running as your OS user could, in principle, ignore its instructions. The protections are the tool-side approval record, the read-back, per-item binding, 24-hour expiry, and eval tests. They are not cryptographic guarantees. Advanced users can approve in the terminal review screen instead of in chat.
- **Basic mode shares data with your AI provider.** In Basic mode the agent reads the matched email headers (sender, subject, date), your account pages and the review list. The tool asks for consent first. The AccountSweep project itself receives nothing: there are no servers and no telemetry.
- **The Basic local database is not encrypted.** It's stored in your user folder with user-only permissions; turn on disk encryption (BitLocker/FileVault) or use the Advanced encrypted build.
- **A clean result is not a guarantee.** "No known breach" only means no public breach is known to the sources checked.
