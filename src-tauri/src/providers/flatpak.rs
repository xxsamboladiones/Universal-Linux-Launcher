use std::process::Command;

use crate::{
    core::model::{ItemKind, LibraryItem, ProviderKind},
    error::{LauncherError, Result},
};

use super::LibraryProvider;

pub struct FlatpakProvider;

impl LibraryProvider for FlatpakProvider {
    fn name(&self) -> &'static str {
        "flatpak"
    }

    fn is_available(&self) -> bool {
        Command::new("flatpak").arg("--version").output().is_ok()
    }

    fn scan(&self) -> Result<Vec<LibraryItem>> {
        let output = Command::new("flatpak")
            .args(["list", "--app", "--columns=application,name"])
            .output()?;
        if !output.status.success() {
            return Err(LauncherError::ProviderUnavailable(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(parse_list(&String::from_utf8_lossy(&output.stdout)))
    }
}

fn parse_list(value: &str) -> Vec<LibraryItem> {
    value
        .lines()
        .filter_map(|line| {
            let (app_id, name) = line.split_once('\t')?;
            if app_id.trim().is_empty() {
                return None;
            }
            let mut item = LibraryItem::new(
                format!("flatpak:{}", app_id.trim()),
                if name.trim().is_empty() {
                    app_id.trim().into()
                } else {
                    name.trim().into()
                },
                ItemKind::Application,
                ProviderKind::Flatpak,
            );
            item.executable = Some("flatpak".into());
            item.arguments = vec!["run".into(), app_id.trim().into()];
            item.category = Some("Flatpak".into());
            item.icon = super::desktop::resolve_icon(app_id.trim());
            Some(item)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_only_application_rows() {
        let items = parse_list("org.kde.kate\tKate\ncom.valvesoftware.Steam\tSteam\n");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "flatpak:org.kde.kate");
        assert_eq!(items[0].arguments, ["run", "org.kde.kate"]);
    }
}
