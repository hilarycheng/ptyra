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
    let read = read_bounded(
        child.stdout.take().ok_or("Fontconfig output unavailable")?,
        &mut bytes,
        MAX_DISCOVERY_BYTES + 1,
    );
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
        read_bounded(
            File::open(path).map_err(|_| "font file unavailable")?,
            &mut bytes,
            budget.min(MAX_FONT_BYTES) + 1,
        )
        .map_err(|_| "font read failed")?;
        if bytes.len() > budget.min(MAX_FONT_BYTES) {
            return Err("font file exceeds limit");
        }
        FontVec::try_from_vec_and_index(bytes, index).map_err(|_| "unsupported font file")
    })
}

/// FontVec keeps the input Vec's capacity, so cap allocation as well as length.
/// The caller includes one sentinel byte when detecting an oversized source.
fn read_bounded(mut reader: impl Read, bytes: &mut Vec<u8>, limit: usize) -> std::io::Result<()> {
    let mut chunk = [0; 16 * 1024];
    while bytes.len() < limit {
        let available = chunk.len().min(limit - bytes.len());
        let count = match reader.read(&mut chunk[..available]) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        let needed = bytes.len() + count;
        if needed > bytes.capacity() {
            let capacity = needed.max(bytes.capacity().saturating_mul(2)).min(limit);
            bytes
                .try_reserve_exact(capacity - bytes.len())
                .map_err(|_| std::io::ErrorKind::OutOfMemory)?;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    // Discovery/font ownership charges actual retained bytes, not spare Vec capacity.
    *bytes = std::mem::take(bytes).into_boxed_slice().into_vec();
    Ok(())
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
    fn runtime_resource_caps_apply_to_actual_owners() {
        struct Chunked<'a>(&'a [u8]);
        impl Read for Chunked<'_> {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                let count = bytes.len().min(4096);
                self.0.read(&mut bytes[..count])
            }
        }
        // Valid trailing padding crosses read_to_end's geometric growth boundary.
        let mut source = FONT.to_vec();
        source.resize(20_000, 0);
        let fonts = load_candidates(
            "/first\n0\n/second\n0\n/third\n0\n",
            40_000,
            |_, _, budget| {
                let mut bytes = Vec::new();
                read_bounded(Chunked(source.as_slice()), &mut bytes, budget + 1)
                    .map_err(|_| "fixture read failed")?;
                FontVec::try_from_vec(bytes).map_err(|_| "invalid fixture")
            },
        )
        .unwrap();
        assert_eq!(fonts.len(), 2);
        let allocated: usize = fonts
            .into_iter()
            .map(|font| font.into_vec().capacity())
            .sum();
        assert_eq!(
            allocated, 40_000,
            "count actual FontVec allocations, not slice lengths"
        );

        let mut oversized = Vec::new();
        let mut reader = source.as_slice();
        read_bounded(&mut reader, &mut oversized, 10_001).unwrap();
        assert_eq!(oversized.len(), 10_001);
        assert_eq!(oversized.capacity(), 10_001);
        assert_eq!(reader.len(), source.len() - 10_001);
    }

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
