//! Versioned JSON Schemas, embedded in the binary.
//!
//! Each schema describes one piece of the tool ↔ agent contract. They are
//! compiled in so the binary can show them (`accountsweep schemas show`) and,
//! from M1 on, validate everything the agent pipes in.

/// One embedded JSON Schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schema {
    /// Short name used on the command line, e.g. `next.v1`.
    pub name: &'static str,
    /// The schema's `$id`.
    pub id: &'static str,
    /// What the schema is for, in one line.
    pub summary: &'static str,
    /// The schema document itself (JSON text).
    pub json: &'static str,
}

/// All schemas shipped with this version, in a stable order.
pub const ALL: &[Schema] = &[
    Schema {
        name: "envelope.v1",
        id: "urn:accountsweep:schema:envelope:v1",
        summary: "Output envelope of every command run with --json",
        json: include_str!("../schemas/envelope.v1.schema.json"),
    },
    Schema {
        name: "next.v1",
        id: "urn:accountsweep:schema:next:v1",
        summary: "One instruction for the agent from `accountsweep next --agent`",
        json: include_str!("../schemas/next.v1.schema.json"),
    },
    Schema {
        name: "ingest-gmail-headers.v1",
        id: "urn:accountsweep:schema:ingest-gmail-headers:v1",
        summary: "Gmail headers piped in by the agent (Basic mode)",
        json: include_str!("../schemas/ingest-gmail-headers.v1.schema.json"),
    },
    Schema {
        name: "ingest-page-items.v1",
        id: "urn:accountsweep:schema:ingest-page-items:v1",
        summary: "Connected apps or devices read from an account page",
        json: include_str!("../schemas/ingest-page-items.v1.schema.json"),
    },
    Schema {
        name: "recipe.v1",
        id: "urn:accountsweep:schema:recipe:v1",
        summary: "Site recipe (TOML) describing a logout/revoke/delete action",
        json: include_str!("../schemas/recipe.v1.schema.json"),
    },
];

/// Look up a schema by its short name (e.g. `next.v1`).
pub fn get(name: &str) -> Option<&'static Schema> {
    ALL.iter().find(|s| s.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_lookup_works() {
        for (i, a) in ALL.iter().enumerate() {
            for b in &ALL[i + 1..] {
                assert_ne!(a.name, b.name, "duplicate schema name");
                assert_ne!(a.id, b.id, "duplicate schema id");
            }
            assert_eq!(get(a.name), Some(a));
        }
        assert_eq!(get("does-not-exist"), None);
    }
}
