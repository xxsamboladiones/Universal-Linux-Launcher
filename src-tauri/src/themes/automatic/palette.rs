use crate::{
    error::{LauncherError, Result},
    themes::manifest::{Colors, Effects, Radius, Spacing, ThemeTokens, Typography},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorPalette {
    pub background: String,
    pub background_alt: String,
    pub surface: String,
    pub surface_elevated: String,
    pub foreground: String,
    pub foreground_muted: String,
    pub primary: String,
    pub secondary: String,
    pub accent: String,
    pub border: String,
    pub success: String,
    pub warning: String,
    pub error: String,
    pub primary_foreground: String,
    pub secondary_foreground: String,
    pub accent_foreground: String,
    pub dark: bool,
}

#[derive(Clone, Copy)]
struct Rgb(u8, u8, u8);
impl Rgb {
    fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
    fn lum(self) -> f32 {
        let c = |v: u8| {
            let x = v as f32 / 255.0;
            if x <= 0.04045 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * c(self.0) + 0.7152 * c(self.1) + 0.0722 * c(self.2)
    }
    fn mix(self, other: Self, amount: f32) -> Self {
        let f = |a, b| (a as f32 + (b as f32 - a as f32) * amount).round() as u8;
        Self(f(self.0, other.0), f(self.1, other.1), f(self.2, other.2))
    }
}
fn parse(value: &str) -> Result<Rgb> {
    let s = value
        .strip_prefix('#')
        .ok_or_else(|| LauncherError::InvalidTheme("cor automática inválida".into()))?;
    if s.len() != 6 {
        return Err(LauncherError::InvalidTheme(
            "cor automática inválida".into(),
        ));
    }
    let n = |a| {
        u8::from_str_radix(&s[a..a + 2], 16)
            .map_err(|_| LauncherError::InvalidTheme("cor automática inválida".into()))
    };
    Ok(Rgb(n(0)?, n(2)?, n(4)?))
}
fn contrast(a: Rgb, b: Rgb) -> f32 {
    let (a, b) = (a.lum(), b.lum());
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
fn readable(background: Rgb, preferred: Rgb) -> Rgb {
    if contrast(background, preferred) >= 4.5 {
        return preferred;
    }
    if contrast(background, Rgb(255, 255, 255)) >= contrast(background, Rgb(0, 0, 0)) {
        Rgb(255, 255, 255)
    } else {
        Rgb(0, 0, 0)
    }
}
pub fn normalized(
    background: &str,
    primary: &str,
    secondary: &str,
    influence: u8,
    mode: &str,
) -> Result<ColorPalette> {
    let raw = parse(background)?;
    let p = parse(primary)?;
    let s = parse(secondary)?;
    let dark = match mode {
        "dark" => true,
        "light" => false,
        _ => raw.lum() < 0.42,
    };
    let base = if dark {
        raw.mix(Rgb(8, 11, 16), 1.0 - influence as f32 / 100.0)
    } else {
        raw.mix(Rgb(248, 250, 252), 1.0 - influence as f32 / 100.0)
    };
    let foreground = readable(
        base,
        if dark {
            Rgb(244, 245, 248)
        } else {
            Rgb(22, 32, 51)
        },
    );
    let muted = base.mix(foreground, 0.55);
    let surface = base.mix(foreground, if dark { 0.07 } else { 0.035 });
    let elevated = base.mix(foreground, if dark { 0.12 } else { 0.07 });
    let border = base.mix(foreground, 0.18);
    let primary_fg = readable(p, foreground);
    let secondary_fg = readable(s, foreground);
    Ok(ColorPalette {
        background: base.hex(),
        background_alt: surface.hex(),
        surface: surface.hex(),
        surface_elevated: elevated.hex(),
        foreground: foreground.hex(),
        foreground_muted: muted.hex(),
        primary: p.hex(),
        secondary: s.hex(),
        accent: p.mix(s, 0.5).hex(),
        border: border.hex(),
        success: "#4ade80".into(),
        warning: "#facc15".into(),
        error: "#f87171".into(),
        primary_foreground: primary_fg.hex(),
        secondary_foreground: secondary_fg.hex(),
        accent_foreground: readable(p.mix(s, 0.5), foreground).hex(),
        dark,
    })
}
impl ColorPalette {
    pub fn tokens(&self) -> ThemeTokens {
        ThemeTokens {
            colors: Colors {
                background: self.background.clone(),
                surface: self.surface.clone(),
                surface_elevated: self.surface_elevated.clone(),
                primary: self.primary.clone(),
                secondary: self.secondary.clone(),
                text: self.foreground.clone(),
                text_muted: self.foreground_muted.clone(),
                border: self.border.clone(),
                success: self.success.clone(),
                warning: self.warning.clone(),
                error: self.error.clone(),
            },
            radius: Radius {
                small: "6px".into(),
                medium: "10px".into(),
                large: "16px".into(),
            },
            spacing: Spacing { unit: "4px".into() },
            typography: Typography {
                font_family: "Inter, system-ui, sans-serif".into(),
                heading_weight: 700,
                body_weight: 400,
            },
            effects: Effects {
                blur: "12px".into(),
                shadow: "0 8px 32px rgba(0,0,0,0.35)".into(),
            },
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn black_and_white_remain_readable() {
        for bg in ["#000000", "#ffffff", "#ee0000", "#0055ff"] {
            let p = normalized(bg, "#8b5cf6", "#22c55e", 100, "automatic").unwrap();
            assert!(contrast(parse(&p.background).unwrap(), parse(&p.foreground).unwrap()) >= 4.5)
        }
    }
}
