#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Authentication failed: {message}")]
    Auth {
        message: String,
        error_code: Option<i32>,
    },

    #[error("MFA verification required")]
    MfaRequired {
        mfa_type: Option<String>,
        email: Option<String>,
    },

    #[error("Token expired: {message}")]
    TokenExpired {
        message: String,
        error_code: Option<i32>,
    },

    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("Device offline: {0}")]
    DeviceOffline(String),

    #[error("API error: {message}")]
    Api {
        message: String,
        error_code: Option<i32>,
    },

    #[error("Not authenticated. Run 'tplc login' first.")]
    NotAuthenticated,

    #[error("Keychain error: {0}")]
    Keychain(String),

    #[error("Device does not support this operation: {0}")]
    UnsupportedOperation(String),

    #[error("{0}")]
    InvalidInput(String),

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl AppError {
    pub fn exit_code(&self) -> i32 {
        match self {
            AppError::Auth { .. }
            | AppError::MfaRequired { .. }
            | AppError::TokenExpired { .. }
            | AppError::NotAuthenticated => 2,
            AppError::DeviceNotFound(_) => 3,
            AppError::DeviceOffline(_) => 4,
            _ => 1,
        }
    }

    pub fn error_type(&self) -> &'static str {
        match self {
            AppError::Auth { .. } => "auth",
            AppError::MfaRequired { .. } => "mfa_required",
            AppError::TokenExpired { .. } => "token_expired",
            AppError::NotAuthenticated => "not_authenticated",
            AppError::DeviceNotFound(_) => "device_not_found",
            AppError::DeviceOffline(_) => "device_offline",
            AppError::Api { .. } => "api",
            AppError::Keychain(_) => "keychain",
            AppError::UnsupportedOperation(_) => "unsupported_operation",
            AppError::InvalidInput(_) => "invalid_input",
            AppError::Http(_) => "http",
            AppError::Json(_) => "json",
            AppError::Io(_) => "io",
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        let mut obj = serde_json::json!({
            "error": self.error_type(),
            "message": self.to_string(),
        });
        if let Some(code) = self.api_error_code() {
            obj["error_code"] = serde_json::json!(code);
        }
        obj
    }

    fn api_error_code(&self) -> Option<i32> {
        match self {
            AppError::Auth { error_code, .. }
            | AppError::TokenExpired { error_code, .. }
            | AppError::Api { error_code, .. } => *error_code,
            _ => None,
        }
    }
}
