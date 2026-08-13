use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub struct ScanReport {
    pub found: usize,
    pub added: usize,
    pub updated: usize,
    pub unavailable: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub provider: String,
    pub status: String,
    pub found: usize,
}
