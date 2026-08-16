pub mod automatic;
pub mod commands;
mod manager;
pub mod manifest;
mod storage;
mod validation;

pub use manager::ThemeManager;
pub use manifest::{ThemeDetails, ThemeSummary};
