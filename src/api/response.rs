use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ApiResponse {
    pub error_code: i32,
    pub result: Option<serde_json::Value>,
    pub msg: Option<String>,
}

impl ApiResponse {
    pub fn successful(&self) -> bool {
        self.error_code == 0
    }
}
