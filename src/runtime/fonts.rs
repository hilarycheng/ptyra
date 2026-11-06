//! Bounded, startup-only Fontconfig discovery. PTY text never reaches Fontconfig.
use super::Result;
use ab_glyph::FontVec;
use std::{
    fs::File,
    io::Read,
    process::{Command, Stdio},
};

pub(super) const MAX_FONTS: usize = 16;
const MAX_CANDIDATES: usize = 64;
const MAX_DISCOVERY_BYTES: usize = 128 * 1024;
const MAX_FONT_BYTES: usize = 32 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 48 * 1024 * 1024;

pub(super) fn load(pattern: &str) -> Result<Vec<FontVec>> {
    let mut child = Command::new("fc-match")
        .args(["-s", "-f", "%{file}\n%{index}\n", "--", pattern])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Fontconfig fc-match unavailable")?;
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .ok_or("Fontconfig output unavailable")?
        .take((MAX_DISCOVERY_BYTES + 1) as u64)
        .read_to_end(&mut bytes);
    let truncated = bytes.len() > MAX_DISCOVERY_BYTES;
    if read.is_err() || truncated {
        let _ = child.kill();
    }
    let status = child.wait().map_err(|_| "Fontconfig lookup failed")?;
    read.map_err(|_| "Fontconfig read failed")?;
    if !truncated && !status.success() {
        return Err("Fontconfig lookup failed");
    }
    bytes.truncate(MAX_DISCOVERY_BYTES);
    // Only complete records survive a bounded read, including non-ASCII paths.
    let end = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |i| i + 1);
    let text = std::str::from_utf8(&bytes[..end]).map_err(|_| "invalid Fontconfig response")?;
    load_candidates(text, MAX_TOTAL_BYTES, |path, index, budget| {
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(|_| "font file unavailable")?
            .take(budget.min(MAX_FONT_BYTES) as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "font read failed")?;
        if bytes.len() > budget.min(MAX_FONT_BYTES) {
            return Err("font file exceeds limit");
        }
        FontVec::try_from_vec_and_index(bytes, index).map_err(|_| "unsupported font file")
    })
}

fn load_candidates(
    text: &str,
    mut budget: usize,
    mut load: impl FnMut(&str, u32, usize) -> Result<FontVec>,
) -> Result<Vec<FontVec>> {
    let mut fonts = Vec::new();
    let mut seen = Vec::new();
    let mut lines = text.lines();
    for _ in 0..MAX_CANDIDATES {
        let (Some(path), Some(index)) = (lines.next(), lines.next()) else {
            break;
        };
        let Ok(index) = index.parse::<u32>() else {
            continue;
        };
        if !path.starts_with('/')
            || path.len() > 4096
            || path.contains('\0')
            || seen.contains(&(path, index))
        {
            continue;
        }
        seen.push((path, index));
        if let Ok(font) = load(path, index, budget) {
            let size = font.as_slice().len();
            if size > budget.min(MAX_FONT_BYTES) {
                continue;
            }
            budget -= size;
            fonts.push(font);
            if fonts.len() == MAX_FONTS || budget == 0 {
                break;
            }
        }
    }
    if fonts.is_empty() {
        Err("no usable font")
    } else {
        Ok(fonts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FONT: &[u8] = include_bytes!("../../tests/fixtures/fallback-primary.ttf");
    #[test]
    fn runtime_font_candidates_bound_discovery_faces_and_bytes() {
        let mut calls = Vec::new();
        let fonts = load_candidates(
            "/broken\n0\n/good\n0\n/good\n0\n/good\n1\nrelative\n0\n/bad\nno\n/truncated\n",
            FONT.len() * 2,
            |path, index, budget| {
                calls.push((path.to_owned(), index, budget));
                if path == "/broken" {
                    return Err("invalid font");
                }
                FontVec::try_from_vec(FONT.to_vec()).map_err(|_| "invalid font")
            },
        )
        .unwrap();
        assert_eq!(fonts.len(), 2);
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[2], ("/good".into(), 1, FONT.len()));
        let text = (0..MAX_CANDIDATES + 10)
            .map(|i| format!("/{i}\n0\n"))
            .collect::<String>();
        let mut calls = 0;
        assert!(
            load_candidates(&text, MAX_TOTAL_BYTES, |_, _, _| {
                calls += 1;
                Err("bad")
            })
            .is_err()
        );
        assert_eq!(calls, MAX_CANDIDATES);
        let fonts = load_candidates(&text, MAX_TOTAL_BYTES, |_, _, _| {
            Ok(FontVec::try_from_vec(FONT.to_vec()).unwrap())
        })
        .unwrap();
        assert_eq!(fonts.len(), MAX_FONTS);
        assert!(
            load_candidates("/good\n0\n", FONT.len() - 1, |_, _, _| Ok(
                FontVec::try_from_vec(FONT.to_vec()).unwrap()
            ))
            .is_err()
        );
    }
}
