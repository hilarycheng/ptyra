use super::{
    Header, invalid,
    transfer::{MAX_IMAGE_BYTES, Transmission},
};
use crate::error::{AppError, Component};
use std::io::{Cursor, ErrorKind};

pub const MAX_IMAGE_PIXELS: usize = 4 * 1024 * 1024;
#[derive(Clone, Copy)]
pub struct ImageLimits {
    bytes: usize,
    pixels: usize,
}
impl Default for ImageLimits {
    fn default() -> Self {
        Self {
            bytes: MAX_IMAGE_BYTES,
            pixels: MAX_IMAGE_PIXELS,
        }
    }
}
impl ImageLimits {
    pub fn new(bytes: usize, pixels: usize) -> Result<Self, AppError> {
        if bytes == 0 || bytes > MAX_IMAGE_BYTES || pixels == 0 || pixels > MAX_IMAGE_PIXELS {
            return Err(invalid());
        }
        Ok(Self { bytes, pixels })
    }
    pub fn bytes(self) -> usize {
        self.bytes
    }
    pub fn check_dimensions(self, width: u32, height: u32) -> Result<usize, AppError> {
        let size = rgba_len(width, height)?;
        if size > self.bytes || size / 4 > self.pixels {
            return Err(AppError::new(Component::Terminal, ErrorKind::FileTooLarge));
        }
        Ok(size)
    }
}
pub struct Image {
    pub(crate) header: Header,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}
impl Image {
    pub fn header(&self) -> &Header {
        &self.header
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn pixels(&self) -> &[u8] {
        &self.rgba
    }
}

pub(crate) fn rgba_len(width: u32, height: u32) -> Result<usize, AppError> {
    let pixels = u64::from(width) * u64::from(height);
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || pixels > MAX_IMAGE_PIXELS as u64
    {
        return Err(invalid());
    }
    Ok(pixels as usize * 4)
}

pub fn decode(transmission: Transmission) -> Result<Image, AppError> {
    decode_with_limits(transmission, ImageLimits::default())
}
pub fn decode_with_limits(
    transmission: Transmission,
    limits: ImageLimits,
) -> Result<Image, AppError> {
    let Transmission { header, bytes, .. } = transmission;
    if bytes.len() > limits.bytes {
        return Err(AppError::new(Component::Terminal, ErrorKind::FileTooLarge));
    }
    header.require_direct()?;
    match header.value(b'f', 32) {
        32 => {
            let width = u32::try_from(header.value(b's', 0)).map_err(|_| invalid())?;
            let height = u32::try_from(header.value(b'v', 0)).map_err(|_| invalid())?;
            if bytes.len() != limits.check_dimensions(width, height)? {
                return Err(invalid());
            }
            Ok(Image {
                header,
                width,
                height,
                rgba: bytes,
            })
        }
        100 => decode_png(header, bytes, limits),
        _ => Err(AppError::new(Component::Terminal, ErrorKind::Unsupported)),
    }
}

fn buffer(size: usize) -> Result<Vec<u8>, AppError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| AppError::new(Component::Terminal, ErrorKind::OutOfMemory))?;
    bytes.resize(size, 0);
    Ok(bytes)
}

fn decode_png(header: Header, bytes: Vec<u8>, limits: ImageLimits) -> Result<Image, AppError> {
    check_png_chunks(&bytes)?;
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(&bytes),
        png::Limits {
            bytes: limits.bytes,
        },
    );
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let info = decoder.read_header_info().map_err(|_| invalid())?;
    let (width, height) = (info.width, info.height);
    let size = limits.check_dimensions(width, height)?;
    if header.get(b's').is_some_and(|w| w != i64::from(width))
        || header.get(b'v').is_some_and(|h| h != i64::from(height))
    {
        return Err(invalid());
    }
    let mut reader = decoder.read_info().map_err(|_| invalid())?;
    let output = reader
        .output_buffer_size()
        .filter(|n| *n <= size)
        .ok_or_else(invalid)?;
    let mut decoded = buffer(output)?;
    let info = reader.next_frame(&mut decoded).map_err(|_| invalid())?;
    reader.finish().map_err(|_| invalid())?;
    if info.bit_depth != png::BitDepth::Eight {
        return Err(invalid());
    }
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        _ => return Err(invalid()),
    };
    if info.buffer_size() != size / 4 * channels {
        return Err(invalid());
    }
    let mut rgba = buffer(size)?;
    for (pixel, out) in decoded.chunks_exact(channels).zip(rgba.chunks_exact_mut(4)) {
        match channels {
            1 => out.copy_from_slice(&[pixel[0], pixel[0], pixel[0], 255]),
            2 => out.copy_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]),
            3 => out.copy_from_slice(&[pixel[0], pixel[1], pixel[2], 255]),
            _ => out.copy_from_slice(pixel),
        }
    }
    Ok(Image {
        header,
        width,
        height,
        rgba,
    })
}

// png 0.18.1 can panic on malformed PLTE lengths; reject them before decoder entry.
fn check_png_chunks(bytes: &[u8]) -> Result<(), AppError> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(invalid());
    }
    let mut offset = 8;
    for _ in 0..4096 {
        let chunk = bytes
            .get(offset..)
            .filter(|bytes| bytes.len() >= 12)
            .ok_or_else(invalid)?;
        let length = u32::from_be_bytes(chunk[..4].try_into().map_err(|_| invalid())?) as usize;
        let end = offset
            .checked_add(length)
            .and_then(|n| n.checked_add(12))
            .filter(|n| *n <= bytes.len())
            .ok_or_else(invalid)?;
        let kind = &chunk[4..8];
        if kind == b"PLTE" && (length == 0 || length > 768 || !length.is_multiple_of(3)) {
            return Err(invalid());
        }
        if matches!(kind, b"acTL" | b"fcTL" | b"fdAT") {
            return Err(AppError::new(Component::Terminal, ErrorKind::Unsupported));
        }
        if kind == b"IEND" {
            return if length == 0 && end == bytes.len() {
                Ok(())
            } else {
                Err(invalid())
            };
        }
        offset = end;
    }
    Err(AppError::new(Component::Terminal, ErrorKind::FileTooLarge))
}

#[cfg(test)]
mod tests {
    use super::super::{GraphicsCommand, Header, transfer::Assembler};
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    #[test]
    fn kitty_image_budget_limits_allocations() {
        let config =
            crate::config::Config::from_toml(Some("image_bytes=4\nimage_pixels=1")).unwrap();
        let limits = config.image_limits().unwrap();
        let command = |data: &[u8]| GraphicsCommand {
            header: Header::parse(b"s=1,v=1").unwrap(),
            payload: STANDARD.encode(data).into_bytes(),
        };
        let mut assembler = Assembler::with_limit(limits.bytes()).unwrap();
        assert!(assembler.push(command(&[1; 5])).is_err());
        let transmission = assembler.push(command(&[1; 4])).unwrap().unwrap();
        assert_eq!(
            decode_with_limits(transmission, limits)
                .unwrap()
                .pixels()
                .len(),
            4
        );
        let transmission = Assembler::default()
            .push(command(&[1; 4]))
            .unwrap()
            .unwrap();
        assert!(decode_with_limits(transmission, ImageLimits::new(3, 1).unwrap()).is_err());
        assert!(limits.check_dimensions(2, 1).is_err());
        for source in [
            "image_bytes=0",
            "image_bytes=16777217",
            "image_pixels=0",
            "image_pixels=4194305",
            "image_pixels=-1",
        ] {
            assert!(crate::config::Config::from_toml(Some(source)).is_err());
        }
    }
    fn image(header: &[u8], bytes: &[u8]) -> Result<Image, AppError> {
        let transmission = Assembler::default()
            .push(GraphicsCommand {
                header: Header::parse(header)?,
                payload: STANDARD.encode(bytes).into_bytes(),
            })?
            .unwrap();
        decode(transmission)
    }
    #[test]
    fn kitty_direct_payload_checks_dimensions() {
        let rgba = image(b"f=32,s=1,v=1", &[1, 2, 3, 4]).unwrap();
        assert_eq!(
            (rgba.width, rgba.height, rgba.rgba.as_slice()),
            (1, 1, &[1, 2, 3, 4][..])
        );
        for header in [
            b"f=32,s=0,v=1".as_slice(),
            b"f=32,s=1",
            b"f=99,s=1,v=1",
            b"f=32,s=4294967295,v=4294967295",
        ] {
            assert!(image(header, &[1, 2, 3, 4]).is_err());
        }
        assert!(image(b"s=2,v=2", &[1, 2, 3, 4]).is_err());
        let png = include_bytes!("../../../tests/fixtures/pixel.png");
        let decoded = image(b"f=100", png).unwrap();
        assert_eq!(decoded.rgba, [1, 2, 3, 4]);
        assert!(image(b"f=100,s=2,v=1", png).is_err());
        assert!(image(b"f=100", &png[..png.len() - 1]).is_err());
        assert!(
            image(
                b"f=100",
                include_bytes!("../../../tests/fixtures/invalid-palette.png")
            )
            .is_err()
        );
    }
}
