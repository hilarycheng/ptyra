//! One actual native palette for rasterization and terminal query replies.
use crate::config::palette::Palette;
use alacritty_terminal::{
    term::color::Colors,
    vte::ansi::{Color, Rgb},
};

pub(crate) fn color(value: Color, colors: &Colors, palette: &Palette) -> u32 {
    let index = match value {
        Color::Spec(rgb) => {
            return (u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b);
        }
        Color::Named(n) => n as usize,
        Color::Indexed(n) => n as usize,
    };
    indexed(index, colors, palette)
}

fn indexed(index: usize, colors: &Colors, palette: &Palette) -> u32 {
    if let Some(rgb) = colors[index] {
        return (u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b);
    }
    match index {
        0..=15 => palette.ansi[index],
        16..=231 => {
            let n = index - 16;
            let level = |v| if v == 0 { 0 } else { 55 + 40 * v as u32 };
            (level(n / 36) << 16) | (level(n / 6 % 6) << 8) | level(n % 6)
        }
        232..=255 => {
            let level = 8 + 10 * (index as u32 - 232);
            level * 0x010101
        }
        257 => palette.background,
        258 => palette.cursor,
        259..=266 => palette.ansi[index - 259],
        _ => palette.foreground,
    }
}

pub(crate) fn query(index: usize, colors: &Colors, palette: &Palette) -> Option<Rgb> {
    if index > 258 {
        return None;
    }
    let rgb = indexed(index, colors, palette);
    Some(Rgb {
        r: (rgb >> 16) as u8,
        g: (rgb >> 8) as u8,
        b: rgb as u8,
    })
}
