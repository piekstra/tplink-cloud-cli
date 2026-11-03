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
//! | `DeviceOffline`, `Api`, `BadStatus`, `Http`, `Json` | `Upstream` | 5   |
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

    /// A status with no cloud answer in it, such as a gateway's 502. The body stays out: a page
    /// like that is anyone's text at any size, and could echo the URL, which holds the token.
    #[error("TP-Link cloud answered HTTP {status}")]
    BadStatus { status: u16 },

    #[error("not logged in; run `tplc auth login`")]
    NotAuthenticated,

    #[error("{0}")]
    UnsupportedOperation(String),

    #[error("{0}")]
    InvalidInput(String),

    /// Without its URL, through the `From` below.
    #[error(transparent)]
    Http(reqwest::Error),

    #[error("parsing JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// A reqwest error prints the URL it was for, and every request to these clouds carries the
/// session token in its query. This is the one way a reqwest error becomes an `AppError`, so
/// no message, `--json` error or tool result can repeat the token. A refused redirect keeps its
/// policy's reason, which holds no URL: TP-Link answered with it and took no command, so it is a
/// failure and not an unknown outcome.
impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_redirect() {
            let reason = std::error::Error::source(&e)
                .map_or_else(|| "refused".to_string(), ToString::to_string);
            return AppError::Api {
                message: format!("TP-Link answered with a redirect: {reason}"),
                error_code: None,
            };
        }
        AppError::Http(e.without_url())
    }
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
            | AppError::BadStatus { .. }
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

    /// A real producer: a client with nothing to connect to, its URL holding a token as the
    /// clouds' requests do.
    #[test]
    fn a_failed_request_does_not_repeat_the_token_in_its_url() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let token = "kasa-token-dummy";
        let refused = runtime
            .block_on(
                reqwest::Client::new()
                    .post(format!("http://127.0.0.1:9/?token={token}"))
                    .send(),
            )
            .unwrap_err();
        assert!(refused.to_string().contains(token), "{refused}");
        let app: AppError = refused.into();
        assert!(!app.to_string().contains(token), "{app}");
        let cli: CliError = app.into();
        assert!(!cli.to_string().contains(token), "{cli}");
        assert_eq!(cli.exit_code(), 5);
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
