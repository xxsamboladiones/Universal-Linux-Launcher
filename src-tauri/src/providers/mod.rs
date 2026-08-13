use crate::{core::model::LibraryItem, error::Result};
pub mod appimage;
pub mod desktop;
pub mod flatpak;
pub mod steam;
pub trait LibraryProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn is_available(&self) -> bool;
    fn scan(&self) -> Result<Vec<LibraryItem>>;
}
pub fn defaults() -> Vec<Box<dyn LibraryProvider>> {
    vec![
        Box::new(steam::SteamProvider),
        Box::new(desktop::DesktopEntryProvider),
        Box::new(flatpak::FlatpakProvider),
        Box::new(appimage::AppImageProvider),
    ]
}
