use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub theme: String,
    /// ID do tema declarativo ativo. `theme` permanece para desserializar
    /// preferências gravadas por versões anteriores do Orbit.
    #[serde(default = "default_active_theme_id")]
    pub active_theme_id: String,
    #[serde(default = "default_theme_mode")]
    pub theme_mode: String,
    #[serde(default = "default_palette_source")]
    pub palette_source: String,
    #[serde(default = "default_wallpaper_influence")]
    pub wallpaper_influence: u8,
    #[serde(default = "default_color_mode")]
    pub automatic_color_mode: String,
    #[serde(default)]
    pub automatic_update: bool,
    #[serde(default)]
    pub manual_wallpaper_path: Option<String>,
    pub scan_on_startup: bool,
    pub confirm_before_remove: bool,
    pub preferred_terminal: Option<String>,
}
fn default_active_theme_id() -> String {
    "orbit-dark".into()
}
fn default_theme_mode() -> String {
    "manual".into()
}
fn default_palette_source() -> String {
    "automatic".into()
}
fn default_wallpaper_influence() -> u8 {
    70
}
fn default_color_mode() -> String {
    "automatic".into()
}
impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            active_theme_id: "orbit-dark".into(),
            theme_mode: default_theme_mode(),
            palette_source: default_palette_source(),
            wallpaper_influence: default_wallpaper_influence(),
            automatic_color_mode: default_color_mode(),
            automatic_update: false,
            manual_wallpaper_path: None,
            scan_on_startup: false,
            confirm_before_remove: true,
            preferred_terminal: Some("konsole".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArgumentPreset {
    pub id: String,
    pub name: String,
    pub arguments: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GamescopeConfig {
    pub enabled: bool,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub output_width: Option<u32>,
    pub output_height: Option<u32>,
    pub fps: Option<u32>,
    pub fullscreen: bool,
    pub upscaler: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CompatibilityConfig {
    pub runtime_id: Option<String>,
    pub prefix_path: Option<String>,
    pub steam_overlay: bool,
    pub gamemode: bool,
    pub mangohud: bool,
    pub gamescope: GamescopeConfig,
    pub dxvk: bool,
    pub vkd3d: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Game,
    Application,
    Script,
    Custom,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Steam,
    Epic,
    Gog,
    Battlenet,
    Desktop,
    Flatpak,
    Appimage,
    Custom,
}

impl ProviderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Steam => "steam",
            Self::Epic => "epic",
            Self::Gog => "gog",
            Self::Battlenet => "battlenet",
            Self::Desktop => "desktop",
            Self::Flatpak => "flatpak",
            Self::Appimage => "appimage",
            Self::Custom => "custom",
        }
    }
    pub fn from_str(value: &str) -> Self {
        match value {
            "steam" => Self::Steam,
            "epic" => Self::Epic,
            "gog" => Self::Gog,
            "battlenet" => Self::Battlenet,
            "desktop" => Self::Desktop,
            "flatpak" => Self::Flatpak,
            "appimage" => Self::Appimage,
            _ => Self::Custom,
        }
    }
}

impl ItemKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Game => "game",
            Self::Application => "application",
            Self::Script => "script",
            Self::Custom => "custom",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItem {
    pub id: String,
    pub name: String,
    pub kind: ItemKind,
    pub provider: ProviderKind,
    pub executable: Option<String>,
    pub arguments: Vec<String>,
    pub working_directory: Option<String>,
    pub environment: HashMap<String, String>,
    pub icon: Option<String>,
    pub cover: Option<String>,
    pub background: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub favorite: bool,
    pub hidden: bool,
    /// Whether the item still belongs to/is exposed by its provider catalog.
    /// This is intentionally separate from `installed`: an Epic entitlement
    /// remains owned after its local files are removed.
    pub owned: bool,
    pub installed: bool,
    pub play_count: u64,
    pub total_play_time_seconds: u64,
    pub last_played_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub terminal: bool,
    pub compatibility: CompatibilityConfig,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemInput {
    pub id: Option<String>,
    pub name: String,
    pub kind: ItemKind,
    pub provider: ProviderKind,
    pub executable: Option<String>,
    #[serde(default)]
    pub arguments: Vec<String>,
    pub working_directory: Option<String>,
    #[serde(default)]
    pub environment: HashMap<String, String>,
    pub icon: Option<String>,
    pub category: Option<String>,
    #[serde(default)]
    pub terminal: bool,
    #[serde(default)]
    pub compatibility: CompatibilityConfig,
}
impl LibraryItem {
    pub fn new(id: String, name: String, kind: ItemKind, provider: ProviderKind) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            id,
            name,
            kind,
            provider,
            executable: None,
            arguments: vec![],
            working_directory: None,
            environment: HashMap::new(),
            icon: None,
            cover: None,
            background: None,
            category: None,
            tags: vec![],
            favorite: false,
            hidden: false,
            owned: true,
            installed: true,
            play_count: 0,
            total_play_time_seconds: 0,
            last_played_at: None,
            created_at: now.clone(),
            updated_at: now,
            terminal: false,
            compatibility: CompatibilityConfig::default(),
        }
    }
}
