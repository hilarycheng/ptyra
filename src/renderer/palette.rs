//! One actual native palette for rasterization and terminal query replies.
use alacritty_terminal::{
    term::color::Colors,
    vte::ansi::{Color, Rgb},
};
pub(crate) const CURSOR: u32 = 0xeeeeec;
const ANSI: [u32; 16] = [
    0x181818, 0xcc0000, 0x4e9a06, 0xc4a000, 0x3465a4, 0x75507b, 0x06989a, 0xd3d7cf, 0x555753,
    0xef2929, 0x8ae234, 0xfce94f, 0x729fcf, 0xad7fa8, 0x34e2e2, 0xeeeeec,
];

pub(crate) fn color(value: Color, colors: &Colors) -> u32 {
    let index = match value {
        Color::Spec(rgb) => {
            return (u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b);
        }
        Color::Named(n) => n as usize,
        Color::Indexed(n) => n as usize,
    };
    indexed(index, colors)
}

fn indexed(index: usize, colors: &Colors) -> u32 {
    if let Some(rgb) = colors[index] {
        return (u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b);
    }
    match index {
        0..=15 => ANSI[index],
        16..=231 => {
            let n = index - 16;
            let level = |v| if v == 0 { 0 } else { 55 + 40 * v as u32 };
            (level(n / 36) << 16) | (level(n / 6 % 6) << 8) | level(n % 6)
        }
        232..=255 => {
            let level = 8 + 10 * (index as u32 - 232);
            level * 0x010101
        }
        257 => 0x181818,
        258 => CURSOR,
        259..=266 => ANSI[index - 259],
        _ => 0xd3d7cf,
    }
}

pub(crate) fn query(index: usize, colors: &Colors) -> Option<Rgb> {
    if index > 258 {
        return None;
    }
    let rgb = indexed(index, colors);
    Some(Rgb {
        r: (rgb >> 16) as u8,
        g: (rgb >> 8) as u8,
        b: rgb as u8,
    })
}
