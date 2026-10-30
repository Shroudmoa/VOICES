//! The palette and the two moods it comes in.
//!
//! The dark theme is lifted from WORDS: a very dark blue page, cyan as the
//! working colour, magenta for trouble, green for good news, amber for a pause
//! or a warning. The light theme is the same language on cool paper, not an
//! inverted screenshot: the accents are darkened until they carry the same
//! weight against a white page.

use clap::ValueEnum;
use ratatui::style::Color;
use serde::{Deserialize, Serialize};

pub type Rgb = [u8; 3];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ThemeName {
    #[default]
    Dark,
    Light,
}

impl ThemeName {
    pub fn label(self) -> &'static str {
        match self {
            ThemeName::Dark => "DARK",
            ThemeName::Light => "LIGHT",
        }
    }

    pub fn toggled(self) -> ThemeName {
        match self {
            ThemeName::Dark => ThemeName::Light,
            ThemeName::Light => ThemeName::Dark,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Rgb,
    pub surface: Rgb,
    pub fg: Rgb,
    pub dim: Rgb,
    pub faint: Rgb,
    pub cyan: Rgb,
    pub magenta: Rgb,
    pub green: Rgb,
    pub amber: Rgb,
    /// Words already spoken.
    pub spoken: Rgb,
    /// Words not yet reached.
    pub upcoming: Rgb,
    /// The current word's background, and the colour printed on it.
    pub current_bg: Rgb,
    pub current_fg: Rgb,
    pub selection_bg: Rgb,
}

impl Palette {
    pub fn of(theme: ThemeName) -> Palette {
        match theme {
            ThemeName::Dark => Palette {
                bg: [0x04, 0x07, 0x0c],
                surface: [0x0b, 0x14, 0x1c],
                fg: [0xcf, 0xf7, 0xff],
                dim: [0x18, 0x2c, 0x38],
                faint: [0x0b, 0x14, 0x1c],
                cyan: [0x00, 0xe5, 0xff],
                magenta: [0xff, 0x2e, 0x97],
                green: [0x2d, 0xff, 0x9c],
                amber: [0xff, 0xb3, 0x3a],
                spoken: mix([0x04, 0x07, 0x0c], [0xcf, 0xf7, 0xff], 0.34),
                upcoming: mix([0x04, 0x07, 0x0c], [0xcf, 0xf7, 0xff], 0.76),
                current_bg: [0x00, 0xe5, 0xff],
                current_fg: [0x04, 0x07, 0x0c],
                selection_bg: mix([0x04, 0x07, 0x0c], [0x00, 0xe5, 0xff], 0.20),
            },
            ThemeName::Light => Palette {
                bg: [0xf3, 0xf3, 0xee],
                surface: [0xe7, 0xe9, 0xe5],
                fg: [0x10, 0x1c, 0x24],
                dim: [0x9d, 0xaa, 0xaf],
                faint: [0xdd, 0xe2, 0xe1],
                cyan: [0x00, 0x7e, 0x99],
                magenta: [0xbb, 0x16, 0x62],
                green: [0x08, 0x7a, 0x4b],
                amber: [0xa2, 0x5d, 0x00],
                spoken: mix([0xf3, 0xf3, 0xee], [0x10, 0x1c, 0x24], 0.44),
                upcoming: mix([0xf3, 0xf3, 0xee], [0x10, 0x1c, 0x24], 0.80),
                current_bg: [0x00, 0x7e, 0x99],
                current_fg: [0xff, 0xff, 0xff],
                selection_bg: mix([0xf3, 0xf3, 0xee], [0x00, 0x7e, 0x99], 0.16),
            },
        }
    }

    /// A colour for "between these two": used for quiet labels and fading
    /// chrome, exactly the way WORDS uses its own mix.
    pub fn quiet(self) -> Rgb {
        mix(self.bg, self.fg, 0.56)
    }
}

pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t).round() as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t).round() as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t).round() as u8,
    ]
}

pub fn color(c: Rgb) -> Color {
    Color::Rgb(c[0], c[1], c[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixing_is_clamped_and_exact_at_the_ends() {
        assert_eq!(mix([0, 0, 0], [10, 20, 30], 0.0), [0, 0, 0]);
        assert_eq!(mix([0, 0, 0], [10, 20, 30], 1.0), [10, 20, 30]);
        assert_eq!(mix([0, 0, 0], [10, 20, 30], 2.0), [10, 20, 30]);
    }

    #[test]
    fn themes_toggle() {
        assert_eq!(ThemeName::Dark.toggled(), ThemeName::Light);
        assert_eq!(ThemeName::Light.toggled(), ThemeName::Dark);
    }
}
