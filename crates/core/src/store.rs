//! Local storage: one SQLite file in the user's data folder.
//!
//! Basic mode keeps the database unencrypted, protected by user-only file
//! permissions (SPEC §11.2); the encrypted build is an Advanced feature.
//! Secure-delete is on, so removed rows are overwritten on disk.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::Result;
use crate::model::{Account, AccountType, site_confidence};
use crate::rules::Category;

/// Database file name inside the data folder.
pub const DB_FILE: &str = "accountsweep.db";

/// Current schema version (SQLite `user_version`).
const SCHEMA_VERSION: i32 = 1;

const MIGRATION_1: &str = "
CREATE TABLE answers (
    key         TEXT PRIMARY KEY,
    value       TEXT NOT NULL,
    answered_at TEXT NOT NULL
) STRICT;

CREATE TABLE accounts (
    id          TEXT PRIMARY KEY,
    site        TEXT NOT NULL,
    domain      TEXT,
    kind        TEXT NOT NULL,
    label       TEXT,
    provider    TEXT,
    confidence  INTEGER NOT NULL DEFAULT 0,
    first_seen  TEXT,
    last_seen   TEXT,
    status      TEXT NOT NULL DEFAULT 'new',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
) STRICT;

CREATE TABLE sightings (
    id            INTEGER PRIMARY KEY,
    account_id    TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    source        TEXT NOT NULL,
    category      TEXT NOT NULL,
    seen_on       TEXT,
    sender_domain TEXT,
    message_hash  TEXT NOT NULL,
    UNIQUE (source, message_hash)
) STRICT;

CREATE INDEX sightings_account ON sightings(account_id);

CREATE TABLE query_runs (
    query_id TEXT PRIMARY KEY,
    items    INTEGER NOT NULL,
    ran_at   TEXT NOT NULL
) STRICT;
";

/// Default data folder: the user's local (non-roaming) app-data directory,
/// e.g. `%LOCALAPPDATA%\AccountSweep\data` on Windows.
pub fn default_home() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "AccountSweep").map(|d| d.data_local_dir().to_path_buf())
}

fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

/// Create the data folder if missing, readable by the user only on Unix.
/// An existing folder chosen by the user is left as is.
fn ensure_data_dir(dir: &Path) -> Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    #[cfg(not(unix))]
    {
        // Windows: %LOCALAPPDATA% is already restricted to the user by default ACLs.
        fs::create_dir_all(dir)?;
    }
    Ok(())
}

fn restrict_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// One email sighting to store.
#[derive(Debug, Clone)]
pub struct NewSighting {
    /// Account the email belongs to.
    pub account_id: String,
    /// Display name used if the account is new.
    pub site: String,
    /// Registrable domain, when known.
    pub domain: Option<String>,
    /// Initial type (fixed for alert items; recomputed for site items).
    pub kind: AccountType,
    /// App or device name (alert items).
    pub label: Option<String>,
    /// Provider (alert items only). Items with a provider keep a fixed type.
    pub provider: Option<String>,
    /// Email category.
    pub category: Category,
    /// Email date (YYYY-MM-DD).
    pub seen_on: Option<String>,
    /// Registrable domain of the sender.
    pub sender_domain: String,
    /// Hash of the message id (the id itself is not stored).
    pub message_hash: String,
}

/// Item counts.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Summary {
    /// Total items.
    pub total: u32,
    /// Items per type.
    pub by_type: BTreeMap<String, u32>,
}

/// Handle to the local database.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (and create or migrate) the database in `home`.
    pub fn open(home: &Path) -> Result<Self> {
        ensure_data_dir(home)?;
        let path = home.join(DB_FILE);
        let conn = Connection::open(&path)?;
        restrict_file(&path)?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "secure_delete", true)?;
        let store = Store { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        let version: i32 = self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 1 {
            let tx = self.conn.unchecked_transaction()?;
            tx.execute_batch(MIGRATION_1)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        }
        Ok(())
    }

    /// A recorded answer.
    pub fn answer(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM answers WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// When an answer was recorded (RFC 3339).
    pub fn answered_at(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT answered_at FROM answers WHERE key = ?1",
                [key],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Record (or replace) an answer.
    pub fn set_answer(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO answers (key, value, answered_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, answered_at = excluded.answered_at",
            params![key, value, now()],
        )?;
        Ok(())
    }

    /// Forget an answer.
    pub fn remove_answer(&self, key: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM answers WHERE key = ?1", [key])?;
        Ok(())
    }

    /// Ids of Gmail queries already ingested.
    pub fn completed_queries(&self) -> Result<BTreeSet<String>> {
        let mut stmt = self.conn.prepare("SELECT query_id FROM query_runs")?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<BTreeSet<_>, _>>()?;
        Ok(ids)
    }

    /// Store a batch of sightings for one query in a single transaction.
    /// Returns how many sightings were new.
    pub fn ingest_batch(&self, query_id: &str, sightings: &[NewSighting]) -> Result<u32> {
        let tx = self.conn.unchecked_transaction()?;
        let ts = now();
        let mut new = 0u32;
        let mut touched = BTreeSet::new();
        for s in sightings {
            tx.execute(
                "INSERT OR IGNORE INTO accounts
                   (id, site, domain, kind, label, provider, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![
                    s.account_id,
                    s.site,
                    s.domain,
                    s.kind.as_str(),
                    s.label,
                    s.provider,
                    ts
                ],
            )?;
            let inserted = tx.execute(
                "INSERT OR IGNORE INTO sightings
                   (account_id, source, category, seen_on, sender_domain, message_hash)
                 VALUES (?1, 'gmail', ?2, ?3, ?4, ?5)",
                params![
                    s.account_id,
                    s.category.as_str(),
                    s.seen_on,
                    s.sender_domain,
                    s.message_hash
                ],
            )?;
            if inserted > 0 {
                new += 1;
                touched.insert(s.account_id.clone());
            }
        }
        for id in &touched {
            refresh_account(&tx, id, &ts)?;
        }
        tx.execute(
            "INSERT INTO query_runs (query_id, items, ran_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(query_id) DO UPDATE SET items = items + excluded.items, ran_at = excluded.ran_at",
            params![query_id, u32::try_from(sightings.len()).unwrap_or(u32::MAX), ts],
        )?;
        tx.commit()?;
        Ok(new)
    }

    /// All items, most confident first.
    pub fn accounts(&self) -> Result<Vec<Account>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, site, domain, kind, label, provider, confidence, first_seen, last_seen, status
             FROM accounts ORDER BY confidence DESC, site COLLATE NOCASE ASC, id ASC",
        )?;
        let rows = stmt
            .query_map([], row_to_account)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|mut a| {
                a.evidence = self.evidence(&a.id)?;
                Ok(a)
            })
            .collect()
    }

    /// One item by id.
    pub fn account(&self, id: &str) -> Result<Option<Account>> {
        let found = self
            .conn
            .query_row(
                "SELECT id, site, domain, kind, label, provider, confidence, first_seen, last_seen, status
                 FROM accounts WHERE id = ?1",
                [id],
                row_to_account,
            )
            .optional()?;
        match found {
            Some(mut a) => {
                a.evidence = self.evidence(&a.id)?;
                Ok(Some(a))
            }
            None => Ok(None),
        }
    }

    fn evidence(&self, id: &str) -> Result<BTreeMap<String, u32>> {
        let mut stmt = self.conn.prepare(
            "SELECT category, COUNT(*) FROM sightings WHERE account_id = ?1 GROUP BY category",
        )?;
        let map = stmt
            .query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))?
            .collect::<std::result::Result<BTreeMap<_, _>, _>>()?;
        Ok(map)
    }

    /// Item counts by type.
    pub fn summary(&self) -> Result<Summary> {
        let mut stmt = self
            .conn
            .prepare("SELECT kind, COUNT(*) FROM accounts GROUP BY kind")?;
        let by_type = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))?
            .collect::<std::result::Result<BTreeMap<_, _>, _>>()?;
        Ok(Summary {
            total: by_type.values().sum(),
            by_type,
        })
    }
}

fn row_to_account(r: &rusqlite::Row<'_>) -> rusqlite::Result<Account> {
    let kind: String = r.get(3)?;
    let confidence: i64 = r.get(6)?;
    Ok(Account {
        id: r.get(0)?,
        site: r.get(1)?,
        domain: r.get(2)?,
        kind: AccountType::parse(&kind).unwrap_or(AccountType::Newsletter),
        label: r.get(4)?,
        provider: r.get(5)?,
        confidence: u8::try_from(confidence.clamp(0, 100)).unwrap_or(0),
        first_seen: r.get(7)?,
        last_seen: r.get(8)?,
        evidence: BTreeMap::new(),
        status: r.get(9)?,
    })
}

/// Recompute type (site items), confidence and dates from sightings.
fn refresh_account(tx: &rusqlite::Transaction<'_>, id: &str, ts: &str) -> Result<()> {
    let (provider, label): (Option<String>, Option<String>) = tx.query_row(
        "SELECT provider, label FROM accounts WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let categories: Vec<Category> = {
        let mut stmt =
            tx.prepare("SELECT DISTINCT category FROM sightings WHERE account_id = ?1")?;
        stmt.query_map([id], |r| r.get::<_, String>(0))?
            .filter_map(|c| c.ok().and_then(|c| Category::parse(&c)))
            .collect()
    };
    let (first, last): (Option<String>, Option<String>) = tx.query_row(
        "SELECT MIN(seen_on), MAX(seen_on) FROM sightings WHERE account_id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if provider.is_some() {
        // Alert items keep their type; a named app is stronger evidence.
        let confidence = if label.is_some() { 85 } else { 50 };
        tx.execute(
            "UPDATE accounts SET confidence = ?2, first_seen = ?3, last_seen = ?4, updated_at = ?5 WHERE id = ?1",
            params![id, confidence, first, last, ts],
        )?;
    } else {
        let kind = AccountType::from_categories(&categories);
        let confidence = site_confidence(&categories);
        tx.execute(
            "UPDATE accounts SET kind = ?2, confidence = ?3, first_seen = ?4, last_seen = ?5, updated_at = ?6 WHERE id = ?1",
            params![id, kind.as_str(), confidence, first, last, ts],
        )?;
    }
    Ok(())
}

/// Delete all local data in `home`. Returns whether a database existed.
pub fn purge(home: &Path) -> Result<bool> {
    let db = home.join(DB_FILE);
    let existed = db.exists();
    for suffix in ["", "-journal", "-wal", "-shm"] {
        let path = home.join(format!("{DB_FILE}{suffix}"));
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    // Remove the folder only if nothing else is in it.
    let _ = fs::remove_dir(home);
    Ok(existed)
}
