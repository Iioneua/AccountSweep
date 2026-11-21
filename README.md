# AccountSweep

> Find the accounts tied to your email, check whether those sites were hit by public data breaches, then **choose** what to log out of or revoke. Your AI agent does the legwork; you make every decision.

**Status:** pre-alpha. The design is set ([SPEC.md](SPEC.md)); the code is being written. Nothing is usable yet.

---

## How it works

AccountSweep has two parts:

- **A local tool** (`accountsweep`, one Rust binary). It does all the work that code can do on its own: grouping your accounts, matching them against breach data, ranking news, recommending actions, tracking what you approved and keeping an audit log. It runs on your machine and keeps its data in an encrypted local database.
- **An agent skill** (`skill/`). It teaches an AI agent, starting with Claude Code, to drive the tool and to do the few things code can't: talk with you, search your Gmail through its connector, read your account pages in your browser, and click through the log-outs and revokes you approved.

The tool leads the workflow and tells the agent one step at a time what to do. So the agent spends few tokens, and the process is the same on every run.

```
 agent's connectors           local tool                       you              agent in your browser
 ──────────────────           ──────────                       ───              ─────────────────────
 Gmail search  ───┐
 connected apps ──┼─► group ─► breach check ─► recommendations ─► approve ─► log out / revoke, one site at a time
 devices pages ───┘           (HIBP, news)                       (chat)     └─► verified, logged, reported
```

## Basic use (default): nothing to set up

You need an agent with a **Gmail connector** and a **browser connector**, for example Claude Code with Claude in Chrome. Then install the skill and ask:

> "Check which accounts use my email and whether any were breached."

The agent installs the tool, checks the release checksum, and walks you through it:

1. It searches your Gmail for sign-up, receipt, password-reset and security-alert emails. It uses a fixed set of searches defined by the tool and reads only sender, subject and date.
2. It reads your Google and Microsoft "connected apps" and "devices" pages in your browser, if you agree.
3. The tool groups everything by site, checks each site against [Have I Been Pwned](https://haveibeenpwned.com)'s public breach list (free, no key) and, if you want, recent breach news.
4. You get a short numbered list in chat with a recommendation and a reason for each item. Reply with what you want, e.g. `3 logout, 7 revoke`.
5. The tool reads your choices back. Nothing happens until you say **yes**.
6. The agent does each approved item in a browser window you can watch. It stops and hands over at any sign-in, 2FA, CAPTCHA, payment page, or final "delete account" click.
7. You get a report: what was done (and verified), what failed, and what's left for you (e.g. change your password at a breached site).

No API keys, no OAuth setup, no terminal needed.

**What reaches your AI provider in Basic mode:** the sender, subject and date of the emails the searches match, the review list shown in chat, and the pages the agent opens during actions. AccountSweep itself has no servers and sends no telemetry. If you'd rather keep your email headers away from the AI model entirely, use the Advanced Gmail scan.

## Advanced use (opt-in, one feature at a time)

These need an API key, a setup procedure or a few manual steps, so they're off unless you ask for them:

| Feature | What it adds | What it needs |
|---|---|---|
| **Own Gmail connection** | The tool reads your inbox headers directly over Google's read-only `gmail.metadata` access. Headers never pass through the AI model, and the scan covers the whole inbox | Creating your own Google OAuth client (step-by-step guide, ~10 min) |
| **Per-email breach check** | Checks your *address* (not just the site) against Have I Been Pwned | A paid HIBP API key, stored in your OS keychain |
| **Password-manager import** | Adds sites from a Bitwarden or Chrome/Edge export. The tool drops passwords as it reads, and the agent never opens the file | Exporting the file yourself |
| **Terminal review** | A full-screen terminal view with search, filters (breach date, leaked data type, source, confidence…) and sorting to review and approve | Running `accountsweep review` in your terminal |

## What it will not do

- No dark-web crawling or illegal sources. Only legitimate breach databases and public news.
- No reading email bodies, and no storing, typing or changing your passwords.
- No bypassing CAPTCHAs, bot detection or 2FA. Those steps are always handed to you.
- No acting without your explicit approval, and no "delete account" clicks on your behalf.
- No claiming you're "safe". "No known breach" only means no public breach is known.

## Design principles

1. **Code first.** Anything deterministic is done by the tool, not by the agent's reasoning.
2. **Basic is simple.** The default needs no keys or setup; everything else is Advanced.
3. **Keep data out of the agent** wherever the mode allows. The tool gives the agent IDs, counts and short summaries, not your data.
4. **Well-known, secure methods for every step:**
   - Google sign-in done the standard way for desktop apps (RFC 8252 with PKCE),
   - secrets kept in your OS keychain,
   - an encrypted local database (SQLCipher),
   - TLS for every connection (rustls),
   - a tamper-evident audit log,
   - signed release provenance.
   Details in [SPEC.md §11.1](SPEC.md).
5. **The human decides.** The tool and agent prepare; you approve; only then does anything happen.

## For terminal users

```bash
accountsweep next                  # what to do now (the agent uses: next --agent)
accountsweep list --breached --data-class Passwords --sort breach_date
accountsweep show <id>             # one account in detail
accountsweep review                # terminal review + approval (Advanced)
accountsweep report --format html  # local report
accountsweep log --verify          # audit log, with tamper check
accountsweep purge                 # delete all local data and stored keys
```

The full command list is in [SPEC.md §7](SPEC.md).

## Site recipes

Each site has a small TOML file that describes where its security page is, what to click, and what success looks like. You can add a site without writing Rust.

```toml
site = "dropbox.com"
name = "Dropbox"
last_verified = "2026-11-21"
agent_clicks = true          # false = the agent opens the page, you click

[actions.logout]
url = "https://www.dropbox.com/account/security"
steps = ["Under 'Web browsers', remove every session except the current one."]
success = "Only the current browser session is listed."
stop_if = ["sign_in", "two_factor", "captcha"]
```

Recipes must contain no secrets and must use HTTPS links to the site itself. Check them with `accountsweep recipes validate`.

## Limitations

- Google and Microsoft offer no public API for personal accounts to revoke other apps' access or sign out devices. The agent does it in your browser instead.
- Some sites have no "log out everywhere" option. The fallback is changing your password yourself.
- Websites change, and recipes need maintenance. Each recipe records when it was last checked.
- A site-level breach doesn't mean *your* account was in it. The report says which kind of match it is.
- Breach data is incomplete by nature.

## Roadmap

- [ ] **M0:** scaffold, licenses, CI, schemas
- [ ] **M1:** core, encrypted storage, agent contract, Gmail via connector, grouping, skill v0
- [ ] **M2:** breach + news checks, recommendations, chat review and approval, report
- [ ] **M3:** recipes, browser actions, verification, audit log (**Basic complete**)
- [ ] **M4:** Advanced: own Gmail OAuth, per-email HIBP, password-manager import
- [ ] **M5:** Advanced: terminal review with filters
- [ ] **M6:** skill evals, docs, v0.1 release

Later: local web review page, Outlook, 1Password, French/Arabic, winget/Homebrew.

## Contributing

Ideas, site recipes and security reviews are welcome. Please open an issue before large changes. **Never include real emails, tokens or personal data in issues or pull requests.**

## Disclaimer

AccountSweep is provided as is, with no guarantee. It can't make your accounts safe and may fail on some sites. "No known breach" only means no public breach is known to the sources checked. Review every action before approving it; you are responsible for the accounts you act on.

## License

Dual-licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
