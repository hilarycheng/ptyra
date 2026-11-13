//! Optional EGL 1.5 / GLES 3 transfer PoC. Safety boundary: DEPENDENCIES.md TASK-197.
use super::{Context, Frame, Result};
use std::{
    ffi::{CStr, c_char, c_void},
    ptr,
};

#[link(name = "EGL")]
unsafe extern "C" {
    fn eglGetPlatformDisplay(
        platform: u32,
        native: *mut c_void,
        attrs: *const isize,
    ) -> *mut c_void;
    fn eglInitialize(display: *mut c_void, major: *mut i32, minor: *mut i32) -> u32;
    fn eglBindAPI(api: u32) -> u32;
    fn eglChooseConfig(
        display: *mut c_void,
        attrs: *const i32,
        configs: *mut *mut c_void,
        size: i32,
        count: *mut i32,
    ) -> u32;
    fn eglCreatePbufferSurface(
        display: *mut c_void,
        config: *mut c_void,
        attrs: *const i32,
    ) -> *mut c_void;
    fn eglCreateContext(
        display: *mut c_void,
        config: *mut c_void,
        share: *mut c_void,
        attrs: *const i32,
    ) -> *mut c_void;
    fn eglMakeCurrent(
        display: *mut c_void,
        draw: *mut c_void,
        read: *mut c_void,
        context: *mut c_void,
    ) -> u32;
    fn eglDestroyContext(display: *mut c_void, context: *mut c_void) -> u32;
    fn eglDestroySurface(display: *mut c_void, surface: *mut c_void) -> u32;
    fn eglTerminate(display: *mut c_void) -> u32;
}
#[link(name = "GLESv2")]
unsafe extern "C" {
    fn glGetString(name: u32) -> *const u8;
    fn glGetError() -> u32;
    fn glGetIntegerv(name: u32, data: *mut i32);
    fn glGenTextures(count: i32, textures: *mut u32);
    fn glDeleteTextures(count: i32, textures: *const u32);
    fn glBindTexture(target: u32, texture: u32);
    fn glTexParameteri(target: u32, name: u32, value: i32);
    fn glTexImage2D(
        target: u32,
        level: i32,
        internal: i32,
        width: i32,
        height: i32,
        border: i32,
        format: u32,
        kind: u32,
        data: *const c_void,
    );
    fn glTexSubImage2D(
        target: u32,
        level: i32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        format: u32,
        kind: u32,
        data: *const c_void,
    );
    fn glGenFramebuffers(count: i32, framebuffers: *mut u32);
    fn glDeleteFramebuffers(count: i32, framebuffers: *const u32);
    fn glBindFramebuffer(target: u32, framebuffer: u32);
    fn glFramebufferTexture2D(
        target: u32,
        attachment: u32,
        texture_target: u32,
        texture: u32,
        level: i32,
    );
    fn glCheckFramebufferStatus(target: u32) -> u32;
    fn glBlitFramebuffer(
        sx0: i32,
        sy0: i32,
        sx1: i32,
        sy1: i32,
        dx0: i32,
        dy0: i32,
        dx1: i32,
        dy1: i32,
        mask: u32,
        filter: u32,
    );
    fn glReadPixels(
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        format: u32,
        kind: u32,
        data: *mut c_void,
    );
    fn glFinish();
}

const TEXTURE: u32 = 0x0de1;
const FRAMEBUFFER: u32 = 0x8d40;
const READ: u32 = 0x8ca8;
const DRAW: u32 = 0x8ca9;
const RGBA: u32 = 0x1908;
const BYTE: u32 = 0x1401;
const NEAREST: u32 = 0x2600;

pub(crate) struct Native {
    display: *mut c_void,
    surface: *mut c_void,
    context: *mut c_void,
    initialized: bool,
    current: bool,
    textures: [u32; 2],
    framebuffers: [u32; 2],
}

impl Native {
    pub(crate) fn new() -> Result<Self> {
        let mut this = Self {
            display: ptr::null_mut(),
            surface: ptr::null_mut(),
            context: ptr::null_mut(),
            initialized: false,
            current: false,
            textures: [0; 2],
            framebuffers: [0; 2],
        };
        // SAFETY: fixed terminated attributes, valid output pointers, owned handles on this thread.
        unsafe {
            this.display = eglGetPlatformDisplay(0x31dd, ptr::null_mut(), ptr::null()); // Mesa surfaceless.
            if this.display.is_null()
                || eglInitialize(this.display, ptr::null_mut(), ptr::null_mut()) == 0
            {
                return Err("EGL initialization failed");
            }
            this.initialized = true;
            let attrs = [
                0x3033, 1, 0x3040, 0x40, 0x3024, 8, 0x3023, 8, 0x3022, 8, 0x3021, 8, 0x3038,
            ];
            let mut config = ptr::null_mut();
            let mut count = 0;
            if eglBindAPI(0x30a0) == 0
                || eglChooseConfig(this.display, attrs.as_ptr(), &mut config, 1, &mut count) == 0
                || count != 1
            {
                return Err("EGL GLES3 config unavailable");
            }
            this.surface = eglCreatePbufferSurface(
                this.display,
                config,
                [0x3057, 1, 0x3056, 1, 0x3038].as_ptr(),
            );
            this.context = eglCreateContext(
                this.display,
                config,
                ptr::null_mut(),
                [0x3098, 3, 0x3038].as_ptr(),
            );
            if this.surface.is_null()
                || this.context.is_null()
                || eglMakeCurrent(this.display, this.surface, this.surface, this.context) == 0
            {
                return Err("EGL context unavailable");
            }
            this.current = true;
            for name in [0x1f00, 0x1f01, 0x1f02] {
                // Vendor, renderer, version; never payload.
                let value = glGetString(name);
                if !value.is_null() {
                    let value = CStr::from_ptr(value.cast::<c_char>()).to_string_lossy();
                    println!(
                        "DEVICE {}",
                        value
                            .chars()
                            .filter(|c| !c.is_control())
                            .take(512)
                            .collect::<String>()
                    );
                }
            }
        }
        Ok(this)
    }

    fn check(&self) -> Result<()> {
        // SAFETY: all callers hold this thread's current context; no memory dereference.
        if unsafe { glGetError() } == 0 {
            Ok(())
        } else {
            Err("GLES probe failed")
        }
    }
}

impl Context for Native {
    fn resize(&mut self, size: (u32, u32)) -> Result<()> {
        // SAFETY: Gpu validates dimensions and caps before these fixed-size allocations.
        unsafe {
            let mut limit = 0;
            glGetIntegerv(0x0d33, &mut limit);
            if limit <= 0 || size.0 > limit as u32 || size.1 > limit as u32 {
                return Err("GPU texture limit");
            }
            glDeleteFramebuffers(2, self.framebuffers.as_ptr());
            glDeleteTextures(2, self.textures.as_ptr());
            self.framebuffers = [0; 2];
            self.textures = [0; 2];
            glGenTextures(2, self.textures.as_mut_ptr());
            glGenFramebuffers(2, self.framebuffers.as_mut_ptr());
            for i in 0..2 {
                glBindTexture(TEXTURE, self.textures[i]);
                glTexParameteri(TEXTURE, 0x2801, NEAREST as i32);
                glTexParameteri(TEXTURE, 0x2800, NEAREST as i32);
                glTexImage2D(
                    TEXTURE,
                    0,
                    0x8058,
                    size.0 as i32,
                    size.1 as i32,
                    0,
                    RGBA,
                    BYTE,
                    ptr::null(),
                );
                glBindFramebuffer(FRAMEBUFFER, self.framebuffers[i]);
                glFramebufferTexture2D(FRAMEBUFFER, 0x8ce0, TEXTURE, self.textures[i], 0);
                if glCheckFramebufferStatus(FRAMEBUFFER) != 0x8cd5 {
                    return Err("GPU framebuffer incomplete");
                }
            }
        }
        self.check()
    }
    fn upload(&mut self, frame: &Frame, pixels: &[u8]) -> Result<()> {
        let stride = frame.size.0 as usize * 4;
        let mut rgba =
            pixels[frame.dirty.start as usize * stride..frame.dirty.end as usize * stride].to_vec();
        for pixel in rgba.chunks_exact_mut(4) {
            let argb = u32::from_ne_bytes(pixel.try_into().map_err(|_| "invalid pixel")?);
            pixel.copy_from_slice(&[
                (argb >> 16) as u8,
                (argb >> 8) as u8,
                argb as u8,
                (argb >> 24) as u8,
            ]);
        }
        // SAFETY: slice covers exactly width * dirty-height * RGBA8; GL copies before returning.
        unsafe {
            glBindTexture(TEXTURE, self.textures[0]);
            glTexSubImage2D(
                TEXTURE,
                0,
                0,
                frame.dirty.start as i32,
                frame.size.0 as i32,
                (frame.dirty.end - frame.dirty.start) as i32,
                RGBA,
                BYTE,
                rgba.as_ptr().cast(),
            );
        }
        self.check()
    }
    fn draw(&mut self, size: (u32, u32)) -> Result<()> {
        // SAFETY: both complete RGBA8 framebuffers belong to this context and have equal bounds.
        unsafe {
            glBindFramebuffer(READ, self.framebuffers[0]);
            glBindFramebuffer(DRAW, self.framebuffers[1]);
            glBlitFramebuffer(
                0,
                0,
                size.0 as i32,
                size.1 as i32,
                0,
                0,
                size.0 as i32,
                size.1 as i32,
                0x4000,
                NEAREST,
            );
            glFinish();
        }
        self.check()
    }
    fn read(&mut self, size: (u32, u32)) -> Result<Vec<u8>> {
        let mut pixels = vec![0; size.0 as usize * size.1 as usize * 4];
        // SAFETY: bounded RGBA8 buffer matches the framebuffer; default pack alignment is four.
        unsafe {
            glBindFramebuffer(READ, self.framebuffers[1]);
            glReadPixels(
                0,
                0,
                size.0 as i32,
                size.1 as i32,
                RGBA,
                BYTE,
                pixels.as_mut_ptr().cast(),
            );
        }
        self.check()?;
        for pixel in pixels.chunks_exact_mut(4) {
            let argb = u32::from_be_bytes([pixel[3], pixel[0], pixel[1], pixel[2]]);
            pixel.copy_from_slice(&argb.to_ne_bytes());
        }
        Ok(pixels)
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        // SAFETY: handles are owned, same-thread, never exposed; partial setup leaves null handles.
        unsafe {
            if self.current {
                glDeleteFramebuffers(2, self.framebuffers.as_ptr());
                glDeleteTextures(2, self.textures.as_ptr());
                eglMakeCurrent(
                    self.display,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
            }
            if !self.context.is_null() {
                eglDestroyContext(self.display, self.context);
            }
            if !self.surface.is_null() {
                eglDestroySurface(self.display, self.surface);
            }
            if self.initialized {
                eglTerminate(self.display);
            }
        }
    }
}
