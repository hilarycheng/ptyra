use super::*;
use crate::{TabId, tabs::TabPty};
struct Fake;
impl TabPty for Fake {
    fn tab(&self) -> TabId {
        TabId(1)
    }
}
#[test]
fn runtime_palette_changes_pixels_and_query_replies() {
    let config = Config::from_toml(Some("[colors]\nforeground='#12ab34'\nbackground='#123456'\ncursor='#abcdef'\n[colors.ansi]\nred='#654321'\n")).unwrap();
    let font =
        FontVec::try_from_vec(include_bytes!("../../../tests/fixtures/demo.ttf").to_vec()).unwrap();
    let mut raster = Raster::new(vec![font], &config).unwrap();
    let mut tabs = config.tab_registry::<Fake>().unwrap();
    let tab = tabs.new_tab(2, 8).unwrap().tab;
    let terminal = tabs.get_mut(tab).unwrap().terminal_mut();
    terminal
        .feed(
            tab,
            b"\x1b[?25l\x1b[41m \x1b[0m\x1b]4;1;?\x07\x1b]10;?\x07\x1b]11;?\x07\x1b]12;?\x07",
        )
        .unwrap();
    let size = (8 * raster.cell_width, 2 * raster.cell_height);
    let mut pixels = vec![0; (size.0 * size.1 * 4) as usize];
    raster
        .draw(terminal, size, &mut pixels, true, true)
        .unwrap();
    assert_eq!(
        u32::from_ne_bytes(pixels[..4].try_into().unwrap()),
        0xff654321
    );
    let offset = raster.cell_width as usize * 4;
    assert_eq!(
        u32::from_ne_bytes(pixels[offset..offset + 4].try_into().unwrap()),
        0xff123456
    );
    assert_eq!(terminal.take_core_replies().0, b"\x1b]4;1;rgb:6565/4343/2121\x07\x1b]10;rgb:1212/abab/3434\x07\x1b]11;rgb:1212/3434/5656\x07\x1b]12;rgb:abab/cdcd/efef\x07");
}

fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> u32 {
    let offset = ((y * width + x) * 4) as usize;
    u32::from_ne_bytes(pixels[offset..offset + 4].try_into().unwrap())
}
fn reply(prefix: &str, rgb: u32) -> String {
    format!(
        "\x1b]{prefix};rgb:{:04x}/{:04x}/{:04x}\x07",
        ((rgb >> 16) & 255) * 257,
        ((rgb >> 8) & 255) * 257,
        (rgb & 255) * 257
    )
}
fn render(raster: &mut Raster, terminal: &Terminal, focused: bool) -> Vec<u8> {
    let size = (32 * raster.cell_width, 4 * raster.cell_height);
    let mut pixels = vec![0; (size.0 * size.1 * 4) as usize];
    raster
        .draw(terminal, size, &mut pixels, focused, true)
        .unwrap();
    pixels
}
#[test]
fn runtime_palette_precedence_reset_cursor_and_overlays() {
    use super::super::{bar, notice, paste};
    let config = Config::from_toml(Some("scrollback_lines=0\nfont_size=20\n[colors]\nforeground='#12ab34'\nbackground='#123456'\ncursor='#abcdef'\n[colors.ansi]\nblack='#102030'\nred='#654321'\nyellow='#345612'\nblue='#563412'\nwhite='#fedcba'\n")).unwrap();
    let palette = config.colors;
    let fonts = [
        include_bytes!("../../../tests/fixtures/fallback-primary.ttf").as_slice(),
        include_bytes!("../../../tests/fixtures/fallback-cjk.ttf").as_slice(),
    ]
    .into_iter()
    .map(|bytes| FontVec::try_from_vec(bytes.to_vec()).unwrap())
    .collect();
    let mut raster = Raster::new(fonts, &config).unwrap();
    let mut tabs = config.tab_registry::<Fake>().unwrap();
    let first = tabs.new_tab(4, 32).unwrap().tab;
    let cw = raster.cell_width;
    let ch = raster.cell_height;
    let width = 32 * cw;
    let terminal = tabs.get_mut(first).unwrap().terminal_mut();
    let mut commands = String::from("\x1b[?25l\x1b[2J\x1b[H");
    let mut replies = String::new();
    for (index, rgb) in palette.ansi.iter().enumerate() {
        let sgr = if index < 8 {
            40 + index
        } else {
            100 + index - 8
        };
        commands.push_str(&format!("\x1b[{sgr}m \x1b]4;{index};?\x07"));
        replies.push_str(&reply(&format!("4;{index}"), *rgb));
    }
    for (index, rgb) in [(16, 0), (231, 0xffffff), (232, 0x080808), (255, 0xeeeeee)] {
        commands.push_str(&format!("\x1b[48;5;{index}m \x1b]4;{index};?\x07"));
        replies.push_str(&reply(&format!("4;{index}"), rgb));
    }
    commands.push_str("\x1b[48;2;36;68;102m \x1b[0mA");
    terminal.feed(first, commands.as_bytes()).unwrap();
    assert_eq!(terminal.take_core_replies().0, replies.as_bytes());
    let pixels = render(&mut raster, terminal, true);
    let expected = palette
        .ansi
        .into_iter()
        .chain([0, 0xffffff, 0x080808, 0xeeeeee, 0x244466]);
    for (column, rgb) in expected.enumerate() {
        assert_eq!(
            pixel(&pixels, width, column as u32 * cw, 0),
            0xff000000 | rgb
        );
    }
    assert!(
        pixels
            .chunks_exact(4)
            .any(|p| u32::from_ne_bytes(p.try_into().unwrap()) == 0xff12ab34)
    );
    terminal.feed(first, b"\x1b]4;1;?;1;#112233;1;?\x07\x1b]10;#445566\x07\x1b]11;#778899\x07\x1b]12;#aabbcc\x07\x1b[2J\x1b[H\x1b[41m \x1b[0mA\x1b[?25h\x1b[2 q\x1b]10;?\x07\x1b]11;?\x07\x1b]12;?\x07").unwrap();
    assert_eq!(
        terminal.take_core_replies().0,
        [
            reply("4;1", palette.ansi[1]),
            reply("4;1", 0x112233),
            reply("10", 0x445566),
            reply("11", 0x778899),
            reply("12", 0xaabbcc)
        ]
        .concat()
        .as_bytes()
    );
    let pixels = render(&mut raster, terminal, true);
    assert_eq!(pixel(&pixels, width, 0, 0), 0xff112233);
    assert_eq!(pixel(&pixels, width, 2 * cw, 0), 0xffaabbcc);
    assert_eq!(pixel(&pixels, width, 3 * cw, 0), 0xff778899);
    assert!(
        pixels
            .chunks_exact(4)
            .any(|p| u32::from_ne_bytes(p.try_into().unwrap()) == 0xff445566)
    );
    // Overrides belong to the first Tab, while every subsequently created Tab inherits config.
    let second = tabs.new_tab(4, 32).unwrap().tab;
    let terminal = tabs.get_mut(second).unwrap().terminal_mut();
    terminal
        .feed(
            second,
            b"\x1b]4;1;?\x07\x1b]10;?\x07\x1b]11;?\x07\x1b]12;?\x07",
        )
        .unwrap();
    assert_eq!(
        terminal.take_core_replies().0,
        [
            reply("4;1", palette.ansi[1]),
            reply("10", palette.foreground),
            reply("11", palette.background),
            reply("12", palette.cursor)
        ]
        .concat()
        .as_bytes()
    );
    let terminal = tabs.get_mut(first).unwrap().terminal_mut();
    terminal.feed(first, b"\x1b]104;1\x07\x1b]110\x07\x1b]111\x07\x1b]112\x07\x1b]4;1;?\x07\x1b]10;?\x07\x1b]11;?\x07\x1b]12;?\x07").unwrap();
    assert_eq!(
        terminal.take_core_replies().0,
        [
            reply("4;1", palette.ansi[1]),
            reply("10", palette.foreground),
            reply("11", palette.background),
            reply("12", palette.cursor)
        ]
        .concat()
        .as_bytes()
    );
    let pixels = render(&mut raster, terminal, true);
    assert_eq!(pixel(&pixels, width, 0, 0), 0xff654321);
    assert_eq!(pixel(&pixels, width, 2 * cw, 0), 0xffabcdef);
    assert_eq!(pixel(&pixels, width, 3 * cw, 0), 0xff123456);
    for reset in [b"\x1b]104\x07".as_slice(), b"\x1b]104;\x07"] {
        terminal
            .feed(first, b"\x1b]4;1;#010203;4;#040506\x07")
            .unwrap();
        terminal.feed(first, reset).unwrap();
        terminal.feed(first, b"\x1b]4;1;?;4;?\x07").unwrap();
        assert_eq!(
            terminal.take_core_replies().0,
            [reply("4;1", palette.ansi[1]), reply("4;4", palette.ansi[4])]
                .concat()
                .as_bytes()
        );
    }
    // RIS preserves upstream OSC overrides; explicit OSC reset reveals config again.
    terminal
        .feed(first, b"\x1b]4;1;#010203\x07\x1bc\x1b]4;1;?\x07")
        .unwrap();
    assert_eq!(
        terminal.take_core_replies().0,
        reply("4;1", 0x010203).as_bytes()
    );
    terminal.feed(first, b"\x1b]104\x07").unwrap();
    // Every cursor shape uses configured color, and blinking-off restores cell colors.
    for code in [2, 4, 6] {
        terminal
            .feed(
                first,
                format!("\x1b[2J\x1b[H\x1b[?25h\x1b[{code} q").as_bytes(),
            )
            .unwrap();
        let pixels = render(&mut raster, terminal, true);
        for y in 0..ch {
            for x in 0..cw {
                let ink = code == 2 || (code == 4 && y == ch - 1) || (code == 6 && x == 0);
                assert_eq!(
                    pixel(&pixels, width, x, y),
                    0xff000000
                        | if ink {
                            palette.cursor
                        } else {
                            palette.background
                        }
                );
            }
        }
        let pixels = render(&mut raster, terminal, false);
        assert_eq!(pixel(&pixels, width, 0, 0), 0xffabcdef);
        assert_eq!(pixel(&pixels, width, 1, 1), 0xff123456);
        let mut pixels = pixels;
        raster
            .draw(terminal, (width, 4 * ch), &mut pixels, true, false)
            .unwrap();
        assert_eq!(pixel(&pixels, width, 0, 0), 0xff123456);
    }
    // A focused block paints glyphs (including a wide glyph) in background ink.
    for (text, span) in [("A", 1), ("中", 2)] {
        terminal
            .feed(
                first,
                format!("\x1b[2J\x1b[H{text}\x1b[H\x1b[2 q").as_bytes(),
            )
            .unwrap();
        let pixels = render(&mut raster, terminal, true);
        assert_eq!(pixel(&pixels, width, span * cw - 1, 0), 0xffabcdef);
        let mut ink = 0;
        for y in 0..ch {
            for x in 0..span * cw {
                let p = pixel(&pixels, width, x, y);
                ink += usize::from(p == 0xff123456);
                assert_ne!(p, 0xff12ab34);
            }
        }
        assert!(ink > 0, "block cursor must preserve a readable glyph");
    }
    // Trusted UI uses the configured ANSI colors, never a child Tab's OSC overrides.
    terminal
        .feed(first, b"\x1b]4;1;#000000;3;#000000;4;#000000\x07")
        .unwrap();
    let layout = bar::Layout::new(&tabs, (width, 4 * ch), (cw, ch), 1);
    let mut pixels = vec![0; (width * 4 * ch * 4) as usize];
    layout
        .draw(
            &tabs,
            &mut raster,
            (width, 4 * ch),
            &mut pixels[..(width * layout.height * 4) as usize],
        )
        .unwrap();
    assert_eq!(pixel(&pixels, width, 0, 0), 0xff563412);
    assert!(paste::draw_prompt(&mut raster, (width, 4 * ch), &mut pixels).unwrap());
    assert_eq!(pixel(&pixels, width, width - 1, 4 * ch - 1), 0xff654321);
    notice::draw(
        &mut raster,
        (width, 4 * ch),
        &mut pixels,
        notice::Notice::Empty,
    )
    .unwrap();
    assert_eq!(pixel(&pixels, width, width - 1, 4 * ch - 1), 0xff345612);
    let mut tiny = vec![0; (cw * ch * 4) as usize];
    notice::draw(&mut raster, (cw, ch), &mut tiny, notice::Notice::Empty).unwrap();
    assert!(
        tiny.chunks_exact(4)
            .all(|p| u32::from_ne_bytes(p.try_into().unwrap()) == 0xff345612)
    );
}
