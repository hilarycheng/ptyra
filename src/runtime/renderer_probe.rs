//! Fixed offscreen replay; does not choose the production renderer.

#[cfg(any(test, feature = "gles-probe"))]
mod gpu;

use super::{Result, damage, raster::Raster, resize_frame_pixels, write_frame_rows};
use crate::{TabId, config::Config, terminal::Terminal};
use ab_glyph::FontVec;
use nix::sys::memfd::{MFdFlags, memfd_create};
use std::{fs::File, io::Write, ops::Range, os::unix::fs::FileExt, time::Instant};

const FRAMES: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Frame {
    sequence: usize,
    size: (u32, u32),
    scale: u32,
    dirty: Range<u32>,
}

trait Backend {
    fn submit(&mut self, frame: Frame, pixels: &[u8]) -> Result<()>;
    fn verify(&mut self, frame: Frame, pixels: &[u8]) -> Result<()>;
}

#[derive(Default)]
struct Shm {
    slots: [Option<File>; 2],
    size: (u32, u32),
    stale: [Option<damage::Band>; 2],
}

impl Backend for Shm {
    fn submit(&mut self, frame: Frame, pixels: &[u8]) -> Result<()> {
        if self.size != frame.size {
            self.slots = [None, None];
            self.stale = [damage::Band::new(0, frame.size.1); 2];
            self.size = frame.size;
        }
        let index = frame.sequence % 2;
        for stale in &mut self.stale {
            damage::Band::add(stale, damage::Band::new(frame.dirty.start, frame.dirty.end));
        }
        let slot = &mut self.slots[index];
        if slot.is_none() {
            let file = File::from(
                memfd_create(c"ptyra-probe", MFdFlags::MFD_CLOEXEC)
                    .map_err(|_| "cannot create probe memory")?,
            );
            file.set_len(pixels.len() as u64)
                .map_err(|_| "cannot size probe memory")?;
            *slot = Some(file);
        }
        if let Some(rows) = self.stale[index] {
            write_frame_rows(
                slot.as_ref().ok_or("missing SHM slot")?,
                pixels,
                frame.size.0,
                rows,
            )?;
            self.stale[index] = None;
        }
        Ok(())
    }

    fn verify(&mut self, frame: Frame, pixels: &[u8]) -> Result<()> {
        let mut actual = vec![0; pixels.len()];
        self.slots[frame.sequence % 2]
            .as_ref()
            .ok_or("missing SHM slot")?
            .read_exact_at(&mut actual, 0)
            .map_err(|_| "SHM readback failed")?;
        if actual != pixels {
            return Err("SHM pixels differ");
        }
        Ok(())
    }
}

// Stable trace fingerprint only; exact byte equality above is the correctness check.
fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn raster() -> Result<Raster> {
    let fonts = [
        include_bytes!("../../tests/fixtures/fallback-primary.ttf").as_slice(),
        include_bytes!("../../tests/fixtures/fallback-cjk.ttf").as_slice(),
        include_bytes!("../../tests/fixtures/fallback-mark.ttf").as_slice(),
    ]
    .into_iter()
    .map(|bytes| FontVec::try_from_vec(bytes.to_vec()).map_err(|_| "invalid fixture font"))
    .collect::<Result<Vec<_>>>()?;
    Raster::new(
        fonts,
        &Config {
            font_size: 20.0,
            ..Config::default()
        },
    )
}

fn replay(backend: &mut impl Backend, rounds: usize, output: &mut impl Write) -> Result<()> {
    if !(1..=1000).contains(&rounds) {
        return Err("rounds must be 1..=1000");
    }
    let mut raster = raster()?;
    let mut reference = self::raster()?;
    let mut pixels = Vec::new();
    let mut expected = Vec::new();
    let mut previous = (0, 0, 0);
    for round in 0..rounds {
        let mut terminal = Terminal::with_scrollback(TabId(1), 18, 35, 32)
            .map_err(|_| "fixture terminal failed")?;
        for phase in 0..FRAMES {
            let scale = if (4..=6).contains(&phase) { 2 } else { 1 };
            let logical = if phase == 6 { (480, 300) } else { (640, 360) };
            let size = super::scale::buffer_size(logical, scale).ok_or("fixture exceeds cap")?;
            raster.set_scale(scale);
            reference.set_scale(scale);
            if previous != (size.0, size.1, scale) || phase == 0 {
                terminal
                    .resize(
                        (size.1 / raster.cell_height) as u16,
                        (size.0 / raster.cell_width) as u16,
                    )
                    .map_err(|_| "fixture resize failed")?;
            }
            let input = match phase {
                0 => "\x1b[2J\x1b[H\x1b[?25lA中e\u{301}\r\n\x1b[31;1mA\x1b[0m\r\nplain",
                1 | 5 => "\x1b[2;2H\x1b[4:3;38;2;31;127;223m中A\x1b[0m",
                2 => "\x1b[1S",
                4 | 6 | 7 => "\x1b[H\x1b[7mA中e\u{301}\x1b[0m",
                _ => "",
            };
            terminal
                .feed(TabId(1), input.as_bytes())
                .map_err(|_| "fixture feed failed")?;
            let grid_rows = terminal.snapshot().ok_or("missing fixture grid")?.rows;
            let dirty = terminal.damage_rows().and_then(|rows| {
                damage::Band::terminal(rows, grid_rows, size.1, raster.cell_height)
            });
            let dirty = if previous != (size.0, size.1, scale) || phase == 0 {
                0..size.1
            } else if input.is_empty() {
                0..0
            } else {
                dirty.map_or(0..0, |rows| rows.range())
            };
            let bytes = size.0 as usize * size.1 as usize * 4;
            resize_frame_pixels(&mut pixels, bytes)?;
            resize_frame_pixels(&mut expected, bytes)?;
            let frame = Frame {
                sequence: round * FRAMES + phase,
                size,
                scale,
                dirty,
            };
            let start = Instant::now();
            raster.draw_rows(
                &terminal,
                size,
                &mut pixels,
                true,
                false,
                frame.dirty.clone(),
            )?;
            let raster_ns = start.elapsed().as_nanos();
            let start = Instant::now();
            backend.submit(frame.clone(), &pixels)?;
            let submit_ns = start.elapsed().as_nanos();
            let start = Instant::now();
            reference.draw_rows(&terminal, size, &mut expected, true, false, 0..size.1)?;
            if pixels != expected {
                return Err("partial raster differs from full frame");
            }
            backend.verify(frame.clone(), &expected)?;
            let verify_ns = start.elapsed().as_nanos();
            writeln!(
                output,
                "FRAME {} {} {} {} {} {} {} {} {} {}",
                frame.sequence,
                scale,
                size.0,
                size.1,
                frame.dirty.start,
                frame.dirty.end,
                digest(&pixels),
                raster_ns,
                submit_ns,
                verify_ns
            )
            .map_err(|_| "probe output failed")?;
            previous = (size.0, size.1, scale);
            terminal.damage_presented();
        }
    }
    Ok(())
}

pub fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: renderer_probe shm|gles ROUNDS");
    }
    let rounds = args[1].parse().map_err(|_| "invalid rounds")?;
    match args[0].as_str() {
        "shm" => {
            println!("PROBE 1 shm");
            replay(&mut Shm::default(), rounds, &mut std::io::stdout().lock())
        }
        #[cfg(feature = "gles-probe")]
        "gles" => {
            println!("PROBE 1 gles");
            let context = gpu::Native::new()?;
            replay(
                &mut gpu::Gpu::new(context),
                rounds,
                &mut std::io::stdout().lock(),
            )
        }
        _ => Err("unknown or disabled renderer probe backend"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_benchmark_replays_identical_frames() {
        #[derive(Default)]
        struct Fake(Vec<(Frame, u64)>);
        impl Backend for Fake {
            fn submit(&mut self, frame: Frame, pixels: &[u8]) -> Result<()> {
                self.0.push((frame, digest(pixels)));
                Ok(())
            }
            fn verify(&mut self, _: Frame, _: &[u8]) -> Result<()> {
                Ok(())
            }
        }
        let mut first = Fake::default();
        let mut second = Fake::default();
        let mut shm = Shm::default();
        replay(&mut first, 2, &mut Vec::new()).unwrap();
        replay(&mut second, 2, &mut Vec::new()).unwrap();
        replay(&mut shm, 2, &mut Vec::new()).unwrap();
        assert_eq!(first.0, second.0);
        assert_eq!(first.0.len(), FRAMES * 2);
        assert!(first.0.iter().any(|(f, _)| f.scale == 2));
        assert!(
            first
                .0
                .iter()
                .any(|(f, _)| f.dirty.end - f.dirty.start < f.size.1)
        );
        assert!(first.0.iter().any(|(f, _)| f.dirty.is_empty()));
        assert!(replay(&mut first, 0, &mut Vec::new()).is_err());
        struct Fails;
        impl Backend for Fails {
            fn submit(&mut self, _: Frame, _: &[u8]) -> Result<()> {
                Err("fixture failure")
            }
            fn verify(&mut self, _: Frame, _: &[u8]) -> Result<()> {
                panic!("failed frame verified")
            }
        }
        assert_eq!(
            replay(&mut Fails, 1, &mut Vec::new()),
            Err("fixture failure")
        );
    }
}
