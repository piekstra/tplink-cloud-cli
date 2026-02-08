use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSet {
    pub token: String,
    pub refresh_token: Option<String>,
    pub username: String,
    pub regional_url: String,
    pub term_id: String,
}
