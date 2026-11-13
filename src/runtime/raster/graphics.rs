use crate::{selection::Screen, terminal::Terminal};

pub(super) fn draw(
    terminal: &Terminal,
    size: (u32, u32),
    cell: (u32, u32),
    pixels: &mut [u8],
    rows: std::ops::Range<u32>,
    behind: bool,
    cursor: Option<(u32, u32, u32, u32)>,
) {
    let Some(store) = terminal.image_store() else {
        return;
    };
    let Some(snapshot) = terminal.snapshot() else {
        return;
    };
    let screen = Screen::from_mode(&snapshot.content.mode);
    let mut placements: Vec<_> = store
        .placements()
        .iter()
        .filter(|p| p.screen == screen && (p.z < 0) == behind)
        .collect();
    placements.sort_by_key(|p| {
        (
            p.z,
            match p.image {
                crate::kitty::graphics::store::ImageId::Named(n) => u64::from(n),
                crate::kitty::graphics::store::ImageId::Anonymous(n) => n,
            },
        )
    });
    for p in placements {
        let Some(image) = store.get(p.image) else {
            continue;
        };
        let sx = |n: u32| u64::from(n) * u64::from(cell.0) / u64::from(p.cell.0);
        let sy = |n: u32| u64::from(n) * u64::from(cell.1) / u64::from(p.cell.1);
        let x = i64::from(p.rect.x) * i64::from(cell.0) + sx(p.offset.0) as i64;
        let y = (i64::from(p.rect.y) + snapshot.content.display_offset as i64) * i64::from(cell.1)
            + sy(p.offset.1) as i64;
        let (w, h) = (sx(p.pixels.0).max(1), sy(p.pixels.1).max(1));
        let left = x.max(0).min(i64::from(size.0)) as u32;
        let top = y.max(i64::from(rows.start)).min(i64::from(size.1)) as u32;
        let right = (x + w as i64).max(0).min(i64::from(size.0)) as u32;
        let bottom = (y + h as i64)
            .max(0)
            .min(i64::from(rows.end))
            .min(i64::from(size.1)) as u32;
        for py in top..bottom {
            for px in left..right {
                if cursor.is_some_and(|(cx, cy, cw, ch)| {
                    px >= cx && px < cx + cw && py >= cy && py < cy + ch
                }) {
                    continue;
                }
                let ix =
                    p.source.x as u64 + (i64::from(px) - x) as u64 * u64::from(p.source.width) / w;
                let iy =
                    p.source.y as u64 + (i64::from(py) - y) as u64 * u64::from(p.source.height) / h;
                let source = (iy * u64::from(image.width()) + ix) as usize * 4;
                let rgba = &image.pixels()[source..source + 4];
                let dest = ((py * size.0 + px) * 4) as usize;
                let old = u32::from_ne_bytes(pixels[dest..dest + 4].try_into().unwrap());
                let a = u32::from(rgba[3]);
                let blend = |channel: u8, shift: u32| {
                    (u32::from(channel) * a + ((old >> shift) & 255) * (255 - a) + 127) / 255
                };
                let color = 0xff000000
                    | (blend(rgba[0], 16) << 16)
                    | (blend(rgba[1], 8) << 8)
                    | blend(rgba[2], 0);
                pixels[dest..dest + 4].copy_from_slice(&color.to_ne_bytes());
            }
        }
    }
}
