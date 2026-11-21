//! The JSON output envelope (`envelope.v1`).
//!
//! Every command run with `--json` prints exactly one envelope on stdout, so
//! agents and scripts can always parse the result the same way.

use serde::Serialize;

/// Value of the `schema` field in every envelope.
pub const ENVELOPE_SCHEMA: &str = "accountsweep.envelope.v1";

/// Stable, machine-readable error codes. Never rename a variant: agents and
/// scripts match on these strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// The requested schema name does not exist.
    UnknownSchema,
    /// An unexpected internal failure.
    Internal,
}

impl ErrorCode {
    /// The exact string written to the `code` field.
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::UnknownSchema => "UNKNOWN_SCHEMA",
            ErrorCode::Internal => "INTERNAL",
        }
    }
}

/// Error details inside a failed envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorBody {
    /// Stable error code.
    pub code: ErrorCode,
    /// Human-readable explanation.
    pub message: String,
}

/// The output envelope.
#[derive(Debug, Clone, Serialize)]
pub struct Envelope<T: Serialize> {
    schema: &'static str,
    command: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ErrorBody>,
    warnings: Vec<String>,
}

impl<T: Serialize> Envelope<T> {
    /// A successful result carrying `data`.
    pub fn ok(command: impl Into<String>, data: T) -> Self {
        Self {
            schema: ENVELOPE_SCHEMA,
            command: command.into(),
            ok: true,
            data: Some(data),
            error: None,
            warnings: Vec::new(),
        }
    }

    /// A failed result.
    pub fn err(command: impl Into<String>, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            schema: ENVELOPE_SCHEMA,
            command: command.into(),
            ok: false,
            data: None,
            error: Some(ErrorBody {
                code,
                message: message.into(),
            }),
            warnings: Vec::new(),
        }
    }

    /// Whether this envelope reports success.
    pub fn is_ok(&self) -> bool {
        self.ok
    }

    /// Add a warning.
    #[must_use]
    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }

    /// Serialize to a single line of JSON.
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_envelope_has_data_and_no_error() {
        let v: serde_json::Value =
            serde_json::to_value(Envelope::ok("version", "0.0.1")).expect("serializes");
        assert_eq!(v["schema"], ENVELOPE_SCHEMA);
        assert_eq!(v["ok"], true);
        assert_eq!(v["data"], "0.0.1");
        assert!(v.get("error").is_none());
    }

    #[test]
    fn err_envelope_uses_stable_code_and_no_data() {
        let env = Envelope::<()>::err("schemas show", ErrorCode::UnknownSchema, "nope");
        let v: serde_json::Value = serde_json::to_value(&env).expect("serializes");
        assert_eq!(v["ok"], false);
        assert_eq!(v["error"]["code"], ErrorCode::UnknownSchema.as_str());
        assert!(v.get("data").is_none());
    }
}
