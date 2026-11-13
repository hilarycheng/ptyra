//! Bounded GPU probe lifecycle, independently testable without an EGL driver.

use super::{Backend, Frame, Result};
#[cfg(feature = "gles-probe")]
mod native;
#[cfg(feature = "gles-probe")]
pub(super) use native::Native;

pub(super) trait Context {
    fn resize(&mut self, size: (u32, u32)) -> Result<()>;
    fn upload(&mut self, frame: &Frame, pixels: &[u8]) -> Result<()>;
    fn draw(&mut self, size: (u32, u32)) -> Result<()>;
    fn read(&mut self, size: (u32, u32)) -> Result<Vec<u8>>;
}

pub(super) struct Gpu<C> {
    context: C,
    size: Option<(u32, u32)>,
    failed: bool,
}
impl<C: Context> Gpu<C> {
    pub(super) fn new(context: C) -> Self {
        Self {
            context,
            size: None,
            failed: false,
        }
    }
}
impl<C: Context> Backend for Gpu<C> {
    fn submit(&mut self, mut frame: Frame, pixels: &[u8]) -> Result<()> {
        let count = u64::from(frame.size.0) * u64::from(frame.size.1);
        if self.failed
            || count == 0
            || count > super::super::MAX_PIXELS as u64
            || pixels.len() as u64 != count * 4
            || frame.dirty.start > frame.dirty.end
            || frame.dirty.end > frame.size.1
            || !(1..=4).contains(&frame.scale)
        {
            return Err("invalid GPU frame or failed context");
        }
        // A failed driver call poisons this probe; Drop still releases its owned context.
        self.failed = true;
        if self.size != Some(frame.size) {
            self.size = None;
            self.context.resize(frame.size)?;
            frame.dirty = 0..frame.size.1;
            self.size = Some(frame.size);
        }
        if !frame.dirty.is_empty() {
            self.context.upload(&frame, pixels)?;
        }
        self.context.draw(frame.size)?;
        self.failed = false;
        Ok(())
    }
    fn verify(&mut self, frame: Frame, pixels: &[u8]) -> Result<()> {
        if self.failed || self.size != Some(frame.size) {
            return Err("GPU frame unavailable");
        }
        self.failed = true;
        if self.context.read(frame.size)? != pixels {
            return Err("GPU pixels differ");
        }
        self.failed = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    #[derive(Default)]
    struct Counts {
        allocations: usize,
        releases: usize,
        uploads: usize,
        live: usize,
    }
    struct Fake {
        counts: Rc<RefCell<Counts>>,
        pixels: Vec<u8>,
        fail: bool,
    }
    impl Context for Fake {
        fn resize(&mut self, size: (u32, u32)) -> Result<()> {
            let mut c = self.counts.borrow_mut();
            if c.live != 0 {
                c.releases += 1;
            }
            c.live = 0;
            if self.fail {
                return Err("fake context lost");
            }
            c.allocations += 1;
            c.live = size.0 as usize * size.1 as usize * 8;
            self.pixels = vec![0; c.live / 2];
            Ok(())
        }
        fn upload(&mut self, frame: &Frame, pixels: &[u8]) -> Result<()> {
            self.counts.borrow_mut().uploads += 1;
            let start = frame.dirty.start as usize * frame.size.0 as usize * 4;
            let end = frame.dirty.end as usize * frame.size.0 as usize * 4;
            self.pixels[start..end].copy_from_slice(&pixels[start..end]);
            Ok(())
        }
        fn draw(&mut self, _: (u32, u32)) -> Result<()> {
            Ok(())
        }
        fn read(&mut self, _: (u32, u32)) -> Result<Vec<u8>> {
            Ok(self.pixels.clone())
        }
    }
    impl Drop for Fake {
        fn drop(&mut self) {
            let mut c = self.counts.borrow_mut();
            if c.live != 0 {
                c.releases += 1;
            }
            c.live = 0;
        }
    }

    #[test]
    fn gpu_renderer_probe_preserves_frame_and_resource_bounds() {
        let counts = Rc::new(RefCell::new(Counts::default()));
        let frame = Frame {
            sequence: 0,
            size: (4, 2),
            scale: 1,
            dirty: 0..1,
        };
        {
            let mut gpu = Gpu::new(Fake {
                counts: counts.clone(),
                pixels: Vec::new(),
                fail: false,
            });
            gpu.submit(frame.clone(), &[255; 32]).unwrap();
            gpu.verify(frame.clone(), &[255; 32]).unwrap(); // Initial upload must cover every row.
            let mut next = vec![255; 32];
            next[..16].fill(1);
            gpu.submit(frame.clone(), &next).unwrap();
            gpu.verify(frame.clone(), &next).unwrap();
            assert_eq!(counts.borrow().allocations, 1);
            assert_eq!(counts.borrow().live, 64);
            let mut invalid = frame.clone();
            invalid.size = (u32::MAX, u32::MAX);
            assert!(gpu.submit(invalid, &[]).is_err());
            let mut invalid = frame.clone();
            invalid.dirty = 0..3;
            assert!(gpu.submit(invalid, &[0; 32]).is_err());
            assert!(gpu.submit(frame.clone(), &[0; 31]).is_err());
            assert_eq!(counts.borrow().uploads, 2);
            gpu.context.fail = true;
            let mut resized = frame.clone();
            resized.size = (2, 2);
            assert!(gpu.submit(resized, &[0; 16]).is_err());
            assert!(gpu.submit(frame.clone(), &[0; 32]).is_err()); // Failed context cannot reuse old storage.
        }
        assert_eq!(counts.borrow().live, 0);
        assert_eq!(counts.borrow().releases, counts.borrow().allocations);
        let mut gpu = Gpu::new(Fake {
            counts,
            pixels: Vec::new(),
            fail: false,
        });
        super::super::replay(&mut gpu, 1, &mut Vec::new()).unwrap();
    }
}
