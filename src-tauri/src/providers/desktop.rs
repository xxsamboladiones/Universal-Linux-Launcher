use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};

use super::LibraryProvider;
use crate::{
    core::model::{ItemKind, LibraryItem, ProviderKind},
    error::{LauncherError, Result},
};

pub struct DesktopEntryProvider;

impl LibraryProvider for DesktopEntryProvider {
    fn name(&self) -> &'static str {
        "desktop"
    }
    fn is_available(&self) -> bool {
        true
    }

    fn scan(&self) -> Result<Vec<LibraryItem>> {
        let mut by_id = HashMap::new();
        for directory in desktop_dirs() {
            let Ok(entries) = fs::read_dir(directory) else {
                continue;
            };
            for entry in entries.flatten() {
                if entry.path().extension().and_then(|value| value.to_str()) != Some("desktop") {
                    continue;
                }
                match parse_file(&entry.path()) {
                    Ok(Some(item)) => {
                        by_id.entry(item.id.clone()).or_insert(item);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        tracing::warn!(error=%error, path=%entry.path().display(), "Desktop Entry ignorada")
                    }
                }
            }
        }
        Ok(by_id.into_values().collect())
    }
}

fn desktop_dirs() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(home) = dirs::home_dir() {
        directories.push(
            env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or(home.join(".local/share"))
                .join("applications"),
        );
    }
    for path in env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".into())
        .split(':')
    {
        directories.push(Path::new(path).join("applications"));
    }
    directories
}

pub fn parse_file(path: &Path) -> Result<Option<LibraryItem>> {
    let text = fs::read_to_string(path)?;
    parse(
        &text,
        path.file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown"),
    )
}

pub fn parse(text: &str, key: &str) -> Result<Option<LibraryItem>> {
    let mut in_desktop_entry = false;
    let mut values = HashMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry || line.starts_with('#') {
            continue;
        }
        if let Some((name, value)) = line.split_once('=') {
            values.entry(name.trim()).or_insert(value.trim());
        }
    }
    if values.get("Type") != Some(&"Application")
        || truthy(values.get("Hidden"))
        || truthy(values.get("NoDisplay"))
    {
        return Ok(None);
    }
    if !visible_on_current_desktop(&values) {
        return Ok(None);
    }
    if let Some(try_exec) = values.get("TryExec") {
        if !executable_exists(try_exec) {
            return Ok(None);
        }
    }

    let name = localized_value(&values, "Name")
        .ok_or_else(|| LauncherError::InvalidDesktopEntry(key.into()))?;
    let exec = values
        .get("Exec")
        .ok_or_else(|| LauncherError::InvalidDesktopEntry(name.clone()))?;
    let parts = parse_exec(exec, &name, key, values.get("Icon").copied())?;
    if parts.is_empty() {
        return Ok(None);
    }
    if delegated_to_native_provider(&parts) {
        return Ok(None);
    }
    let categories = values.get("Categories").copied().unwrap_or("");
    let game = categories.split(';').any(|category| category == "Game");
    let mut item = LibraryItem::new(
        format!("desktop:{key}"),
        name,
        if game {
            ItemKind::Game
        } else {
            ItemKind::Application
        },
        ProviderKind::Desktop,
    );
    item.executable = Some(parts[0].clone());
    item.arguments = parts[1..].to_vec();
    item.icon = values
        .get("Icon")
        .map(|value| resolve_icon(value).unwrap_or_else(|| (*value).to_string()));
    item.category = categories
        .split(';')
        .find(|value| !value.is_empty())
        .map(str::to_string);
    item.working_directory = values.get("Path").map(|value| (*value).to_string());
    item.terminal = truthy(values.get("Terminal"));
    Ok(Some(item))
}

fn delegated_to_native_provider(parts: &[String]) -> bool {
    let executable = Path::new(&parts[0])
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&parts[0]);
    let steam = executable == "steam"
        && parts
            .iter()
            .skip(1)
            .any(|argument| argument.starts_with("steam://rungameid/") || argument == "-applaunch");
    let steam_uri = parts[0].starts_with("steam://rungameid/");
    let flatpak = executable == "flatpak" && parts.get(1).is_some_and(|argument| argument == "run");
    steam || steam_uri || flatpak
}

fn localized_value(values: &HashMap<&str, &str>, field: &str) -> Option<String> {
    if let Ok(locale) = env::var("LC_MESSAGES").or_else(|_| env::var("LANG")) {
        let locale = locale.split('.').next().unwrap_or(&locale);
        for candidate in [locale, locale.split('_').next().unwrap_or(locale)] {
            if let Some(value) = values.get(format!("{field}[{candidate}]").as_str()) {
                return Some((*value).into());
            }
        }
    }
    values.get(field).map(|value| (*value).into())
}

fn visible_on_current_desktop(values: &HashMap<&str, &str>) -> bool {
    let current = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let desktops: Vec<_> = current
        .split(':')
        .filter(|value| !value.is_empty())
        .collect();
    if let Some(only) = values.get("OnlyShowIn") {
        if !only.split(';').any(|value| {
            desktops
                .iter()
                .any(|desktop| desktop.eq_ignore_ascii_case(value))
        }) {
            return false;
        }
    }
    if let Some(excluded) = values.get("NotShowIn") {
        if excluded.split(';').any(|value| {
            desktops
                .iter()
                .any(|desktop| desktop.eq_ignore_ascii_case(value))
        }) {
            return false;
        }
    }
    true
}

fn executable_exists(value: &str) -> bool {
    if value.contains('/') {
        return Path::new(value).is_file();
    }
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|path| path.join(value).is_file()))
}

pub fn resolve_icon(name: &str) -> Option<String> {
    let direct = Path::new(name);
    if direct.is_file() {
        return Some(direct.to_string_lossy().into_owned());
    }
    let mut roots = vec![
        PathBuf::from("/usr/share/icons"),
        PathBuf::from("/usr/share/pixmaps"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.insert(0, home.join(".local/share/icons"));
        roots.insert(1, home.join(".icons"));
    }
    let extensions = ["svg", "png", "webp", "xpm"];
    let names = if Path::new(name).extension().is_some() {
        vec![name.to_string()]
    } else {
        extensions
            .iter()
            .map(|extension| format!("{name}.{extension}"))
            .collect()
    };
    for root in roots {
        if root.ends_with("pixmaps") {
            for filename in &names {
                let path = root.join(filename);
                if path.is_file() {
                    return Some(path.to_string_lossy().into_owned());
                }
            }
            continue;
        }
        for theme in ["hicolor", "breeze", "breeze-dark"] {
            for size in ["scalable", "256x256", "128x128", "64x64", "48x48", "32x32"] {
                for context in ["apps", "applications"] {
                    for filename in &names {
                        for path in [
                            root.join(theme).join(size).join(context).join(filename),
                            root.join(theme).join(context).join(size).join(filename),
                        ] {
                            if path.is_file() {
                                return Some(path.to_string_lossy().into_owned());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

fn truthy(value: Option<&&str>) -> bool {
    value.is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

pub fn parse_exec(
    input: &str,
    display_name: &str,
    desktop_key: &str,
    icon: Option<&str>,
) -> Result<Vec<String>> {
    let mut output = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for character in input.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '"' => quoted = !quoted,
            ' ' | '\t' if !quoted => {
                if !current.is_empty() {
                    push_field(
                        &mut output,
                        std::mem::take(&mut current),
                        display_name,
                        desktop_key,
                        icon,
                    );
                }
            }
            _ => current.push(character),
        }
    }
    if quoted || escaped {
        return Err(LauncherError::InvalidArguments(input.into()));
    }
    if !current.is_empty() {
        push_field(&mut output, current, display_name, desktop_key, icon);
    }
    Ok(output)
}

fn push_field(
    output: &mut Vec<String>,
    value: String,
    display_name: &str,
    desktop_key: &str,
    icon: Option<&str>,
) {
    if matches!(value.as_str(), "%f" | "%F" | "%u" | "%U") {
        return;
    }
    if value == "%i" {
        if let Some(icon) = icon {
            output.extend(["--icon".into(), icon.into()]);
        }
        return;
    }
    let expanded = value
        .replace("%c", display_name)
        .replace("%k", desktop_key)
        .replace("%%", "%");
    if !expanded.is_empty() {
        output.push(expanded);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_quoted_exec_and_placeholders() {
        assert_eq!(
            parse_exec(
                "/opt/My\\ App/run --name \"hello world\" %U %c",
                "Demo",
                "demo.desktop",
                None
            )
            .unwrap(),
            ["/opt/My App/run", "--name", "hello world", "Demo"]
        );
    }
    #[test]
    fn hides_entries() {
        assert!(parse(
            "[Desktop Entry]\nType=Application\nName=X\nExec=x\nNoDisplay=true",
            "x"
        )
        .unwrap()
        .is_none());
    }
    #[test]
    fn reads_path_and_terminal() {
        let item = parse("[Desktop Entry]\nType=Application\nName=X\nExec=/bin/echo ok\nPath=/tmp\nTerminal=true", "x").unwrap().unwrap();
        assert!(item.terminal);
        assert_eq!(item.working_directory.as_deref(), Some("/tmp"));
    }
    #[test]
    fn ignores_shortcuts_owned_by_steam_or_flatpak() {
        for exec in [
            "steam steam://rungameid/330020",
            "steam -applaunch 330020",
            "steam://rungameid/330020 %u",
            "flatpak run io.example.Game",
        ] {
            let entry = format!("[Desktop Entry]\nType=Application\nName=Game\nExec={exec}");
            assert!(parse(&entry, "game").unwrap().is_none(), "{exec}");
        }
    }
}
