//! The vendor client's error type, and its mapping onto the family exit-code
//! contract (`pk_cli_core::CliError`, SPEC v1 §1.5).
//!
//! `AppError` is what `api/` and `models/` raise: it keeps the cloud's own
//! error codes so a message can name them. Everything above that layer works
//! in `CliError`, and the `From` impl below is the single place the mapping
//! lives:
//!
//! | `AppError`                                       | `CliError`   | exit |
//! |--------------------------------------------------|--------------|------|
//! | `Auth`, `MfaRequired`, `TokenExpired`, `NotAuthenticated` | `Auth`  | 3 |
//! | `DeviceNotFound`                                 | `NotFound`   | 4    |
//! | `DeviceOffline`, `Api`, `Http`, `Json`           | `Upstream`   | 5    |
//! | `InvalidInput`, `UnsupportedOperation`           | `Usage`      | 2    |
//! | `Io`                                             | `Other`      | 1    |

use pk_cli_core::CliError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("authentication failed: {message}{}", code_suffix(*.error_code))]
    Auth {
        message: String,
        error_code: Option<i32>,
    },

    #[error("MFA verification required{}", .email.as_deref().map(|e| format!(" for {e}")).unwrap_or_default())]
    MfaRequired {
        mfa_type: Option<String>,
        email: Option<String>,
    },

    #[error("{message}{}", code_suffix(*.error_code))]
    TokenExpired {
        message: String,
        error_code: Option<i32>,
    },

    #[error("{0}")]
    DeviceNotFound(String),

    #[error("device offline: {0}")]
    DeviceOffline(String),

    #[error("TP-Link cloud error: {message}{}", code_suffix(*.error_code))]
    Api {
        message: String,
        error_code: Option<i32>,
    },

    #[error("not logged in; run `tplc auth login`")]
    NotAuthenticated,

    #[error("{0}")]
    UnsupportedOperation(String),

    #[error("{0}")]
    InvalidInput(String),

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error("parsing JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn code_suffix(code: Option<i32>) -> String {
    code.map(|c| format!(" (code {c})")).unwrap_or_default()
}

impl AppError {
    /// The cloud's own error code, when the failure carried one.
    pub fn api_error_code(&self) -> Option<i32> {
        match self {
            AppError::Auth { error_code, .. }
            | AppError::TokenExpired { error_code, .. }
            | AppError::Api { error_code, .. } => *error_code,
            _ => None,
        }
    }
}

impl From<AppError> for CliError {
    fn from(e: AppError) -> Self {
        let msg = e.to_string();
        match e {
            AppError::Auth { .. }
            | AppError::MfaRequired { .. }
            | AppError::TokenExpired { .. }
            | AppError::NotAuthenticated => CliError::Auth(msg),
            AppError::DeviceNotFound(_) => CliError::NotFound(msg),
            AppError::DeviceOffline(_)
            | AppError::Api { .. }
            | AppError::Http(_)
            | AppError::Json(_) => CliError::Upstream(msg),
            AppError::InvalidInput(_) | AppError::UnsupportedOperation(_) => CliError::Usage(msg),
            AppError::Io(_) => CliError::Other(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_follow_the_family_contract() {
        let cases: Vec<(AppError, i32)> = vec![
            (
                AppError::Auth {
                    message: "bad password".into(),
                    error_code: Some(-20601),
                },
                3,
            ),
            (
                AppError::MfaRequired {
                    mfa_type: None,
                    email: None,
                },
                3,
            ),
            (
                AppError::TokenExpired {
                    message: "expired".into(),
                    error_code: Some(-20651),
                },
                3,
            ),
            (AppError::NotAuthenticated, 3),
            (AppError::DeviceNotFound("no device matches `x`".into()), 4),
            (AppError::DeviceOffline("x".into()), 5),
            (
                AppError::Api {
                    message: "boom".into(),
                    error_code: Some(-1),
                },
                5,
            ),
            (AppError::InvalidInput("bad".into()), 2),
            (AppError::UnsupportedOperation("no emeter".into()), 2),
        ];
        for (err, code) in cases {
            let cli: CliError = err.into();
            assert_eq!(cli.exit_code(), code, "{cli}");
        }
    }

    #[test]
    fn messages_keep_the_cloud_error_code() {
        let e: CliError = AppError::Api {
            message: "Parameter doesn't exist".into(),
            error_code: Some(-20104),
        }
        .into();
        assert!(e.to_string().contains("(code -20104)"), "{e}");
        let e: CliError = AppError::NotAuthenticated.into();
        assert!(e.to_string().contains("auth login"), "{e}");
    }
}
