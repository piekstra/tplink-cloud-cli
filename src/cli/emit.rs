//! The per-invocation context, plus the one output shape the shared
//! renderer lacks: a list with a context head (SPEC v1 §1.4).

use pk_cli_config::ConfigStore;
use pk_cli_core::{output, CliError};
use serde_json::Value;

use crate::config::Config;
use crate::error::AppError;
use crate::session::{Sessions, TokenSet};

pub struct Ctx<'a> {
    pub json: bool,
    pub verbose: bool,
    pub quiet: bool,
    /// Prompting is acceptable: stdin is a TTY and no `--json`.
    pub interactive: bool,
    pub store: &'a ConfigStore,
    pub sessions: &'a Sessions,
    pub cfg: Config,
}

impl Ctx<'_> {
    /// The stored session, or exit 3 with a pointer at `auth login`. The one
    /// place credentialed commands touch the keychain.
    pub fn session(&self) -> Result<TokenSet, CliError> {
        self.sessions
            .load()?
            .filter(|t| !t.token.is_empty())
            .ok_or_else(|| AppError::NotAuthenticated.into())
    }
}

/// A list that belongs to something — a device's schedule rules, one
/// month's energy days — carries that context beside `items`. JSON:
/// `{"schema": "<record>-list/v1", ...head, "items": [...]}`; text: the
/// head as a key/value block, then the pipe table `output::emit_list`
/// renders. Plain lists use `pk_cli_core::output::emit_list` directly.
pub fn emit_headed_list(
    json: bool,
    record: &str,
    head: serde_json::Map<String, Value>,
    items: Vec<Value>,
    columns: &[&str],
) {
    let mut payload = head;
    payload.insert("items".into(), Value::Array(items));
    output::emit(json, &format!("{record}-list"), payload.into(), |v| {
        if let Some(map) = v.as_object() {
            let head: serde_json::Map<String, Value> = map
                .iter()
                .filter(|(k, _)| k.as_str() != "items")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            if !head.is_empty() {
                output::kv(&Value::Object(head), 0);
            }
        }
        output::table(&output::table_view(&output::rows_of(v, "items"), columns));
    });
}

/// A device answered a query with nothing. Upstream (exit 5): the request
/// reached the cloud and came back empty, which is the device's problem, not
/// the caller's.
pub fn no_data(what: &str) -> CliError {
    CliError::Upstream(format!("the device returned no {what}"))
}
