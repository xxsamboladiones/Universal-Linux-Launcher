use serde::{Deserialize, Serialize};

pub const THEME_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    #[serde(rename = "type")]
    pub theme_type: ThemeType,
    pub orbit_version: String,
    pub preview: Option<String>,
    pub entry: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThemeType {
    Dark,
    Light,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeTokens {
    pub colors: Colors,
    pub radius: Radius,
    pub spacing: Spacing,
    pub typography: Typography,
    pub effects: Effects,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Colors {
    pub background: String,
    pub surface: String,
    pub surface_elevated: String,
    pub primary: String,
    pub secondary: String,
    pub text: String,
    pub text_muted: String,
    pub border: String,
    pub success: String,
    pub warning: String,
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_foreground: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_foreground: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_foreground: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Radius {
    pub small: String,
    pub medium: String,
    pub large: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spacing {
    pub unit: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Typography {
    pub font_family: String,
    pub heading_weight: u16,
    pub body_weight: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Effects {
    pub blur: String,
    pub shadow: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    #[serde(rename = "type")]
    pub theme_type: ThemeType,
    pub orbit_version: String,
    pub preview_url: Option<String>,
    pub source: ThemeSource,
    pub compatible: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThemeSource {
    Builtin,
    External,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeDetails {
    #[serde(flatten)]
    pub summary: ThemeSummary,
    pub tokens: ThemeTokens,
}
