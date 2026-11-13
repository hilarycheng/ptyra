/// Conservative source; assets/ptyra.terminfo.bin is generated with tic for the PTY adapter.
pub const SOURCE: &str = include_str!("../assets/ptyra.terminfo");
pub const COMPILED: &[u8] = include_bytes!("../assets/ptyra.terminfo.bin");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TabId, terminal::Terminal};
    use alacritty_terminal::{
        index::{Column, Line},
        term::{TermMode, cell::Flags},
        vte::ansi::{Color, NamedColor},
    };

    #[test]
    fn terminfo_does_not_claim_unsupported_features() {
        let source = SOURCE
            .lines()
            .filter(|line| !line.starts_with('#'))
            .collect::<String>();
        let mut fields = source
            .split(',')
            .map(str::trim)
            .filter(|field| !field.is_empty());
        assert_eq!(fields.next(), Some("ptyra|Ptyra conservative terminal"));
        let fields: Vec<_> = fields.collect();
        let allowed = [
            "am", "xenl", "cols", "lines", "colors", "pairs", "clear", "cup", "el", "ed", "cr",
            "ind", "cuu1", "cud1", "cub1", "cuf1", "sgr0", "bold", "rev", "op", "setaf", "setab",
            "smcup", "rmcup",
        ];
        let mut seen = std::collections::BTreeSet::new();
        for field in &fields {
            let name = field.split(['=', '#']).next().unwrap();
            assert!(allowed.contains(&name), "unverified capability: {name}");
            assert!(seen.insert(name), "duplicate capability: {name}");
        }
        assert_eq!(seen.len(), allowed.len());
        assert!(fields.contains(&"colors#16"));
        assert!(fields.contains(&"pairs#256"));
        let value = |name: &str| {
            fields
                .iter()
                .find_map(|field| {
                    field
                        .split_once('=')
                        .filter(|(key, _)| *key == name)
                        .map(|(_, value)| value)
                })
                .unwrap()
        };
        let sequence = |name: &str| {
            value(name)
                .replace("\\E", "\x1b")
                .replace("\\r", "\r")
                .replace("\\n", "\n")
        };
        let mut terminal = Terminal::new(TabId(1), 4, 8).unwrap();
        let send = |terminal: &mut Terminal, bytes: &str| {
            terminal.feed(TabId(1), bytes.as_bytes()).unwrap()
        };
        send(&mut terminal, "main");
        send(&mut terminal, &sequence("smcup"));
        assert!(terminal.mode().unwrap().contains(TermMode::ALT_SCREEN));
        send(&mut terminal, "alternate");
        send(&mut terminal, &sequence("rmcup"));
        assert!(!terminal.mode().unwrap().contains(TermMode::ALT_SCREEN));
        assert_eq!(terminal.grid().unwrap()[Line(0)][Column(0)].c, 'm');
        send(&mut terminal, &sequence("clear"));
        assert!(
            terminal
                .grid()
                .unwrap()
                .display_iter()
                .all(|cell| cell.cell.c == ' ')
        );
        assert_eq!(terminal.grid().unwrap().cursor.point.line, Line(0));
        assert_eq!(terminal.grid().unwrap().cursor.point.column, Column(0));
        assert_eq!(value("cup"), "\\E[%i%p1%d;%p2%dH");
        send(&mut terminal, "\x1b[2;3H");
        assert_eq!(terminal.grid().unwrap().cursor.point.line, Line(1));
        assert_eq!(terminal.grid().unwrap().cursor.point.column, Column(2));
        assert_eq!(value("setaf"), "\\E[%?%p1%{8}%<%t3%p1%d%e9%p1%{8}%-%d%;m");
        assert_eq!(value("setab"), "\\E[%?%p1%{8}%<%t4%p1%d%e10%p1%{8}%-%d%;m");
        assert_eq!(value("op"), "\\E[39;49m");
        assert_eq!(value("bold"), "\\E[1m");
        // Every advertised ANSI color, including bright black, has a distinct core index.
        let names = [
            NamedColor::Black,
            NamedColor::Red,
            NamedColor::Green,
            NamedColor::Yellow,
            NamedColor::Blue,
            NamedColor::Magenta,
            NamedColor::Cyan,
            NamedColor::White,
            NamedColor::BrightBlack,
            NamedColor::BrightRed,
            NamedColor::BrightGreen,
            NamedColor::BrightYellow,
            NamedColor::BrightBlue,
            NamedColor::BrightMagenta,
            NamedColor::BrightCyan,
            NamedColor::BrightWhite,
        ];
        for (index, name) in names.into_iter().enumerate() {
            let fg = if index < 8 {
                30 + index
            } else {
                90 + index - 8
            };
            let bg = if index < 8 {
                40 + index
            } else {
                100 + index - 8
            };
            send(&mut terminal, &format!("\x1b[H\x1b[{fg};{bg}mX"));
            let cell = &terminal.grid().unwrap()[Line(0)][Column(0)];
            assert_eq!(cell.fg, Color::Named(name));
            assert_eq!(cell.bg, Color::Named(name));
        }
        send(&mut terminal, &sequence("bold"));
        send(&mut terminal, &sequence("op"));
        send(&mut terminal, "Y");
        let cell = &terminal.grid().unwrap()[Line(0)][Column(1)];
        assert_eq!(cell.fg, Color::Named(NamedColor::Foreground));
        assert_eq!(cell.bg, Color::Named(NamedColor::Background));
        assert!(
            cell.flags.contains(Flags::BOLD),
            "op resets colors, not attributes"
        );
        send(&mut terminal, &sequence("sgr0"));
        send(&mut terminal, "\x1b[2;3H\x1b[31m\x1b[44m");
        send(&mut terminal, &sequence("rev"));
        send(&mut terminal, "X");
        let cell = &terminal.grid().unwrap()[Line(1)][Column(2)];
        assert_eq!(cell.fg, Color::Named(NamedColor::Red));
        assert_eq!(cell.bg, Color::Named(NamedColor::Blue));
        assert!(cell.flags.contains(Flags::INVERSE));
        send(&mut terminal, &sequence("sgr0"));
        send(&mut terminal, "Y");
        let cell = &terminal.grid().unwrap()[Line(1)][Column(3)];
        assert_eq!(cell.fg, Color::Named(NamedColor::Foreground));
        assert_eq!(cell.bg, Color::Named(NamedColor::Background));
        assert!(!cell.flags.contains(Flags::INVERSE));
        terminal.feed(TabId(1), b"\x1b[>31u\x1b[?u").unwrap();
        assert_eq!(terminal.take_keyboard_reply().unwrap().bytes, b"\x1b[?1u");
    }
}
