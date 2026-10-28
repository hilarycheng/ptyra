use crate::{
    error::{AppError, Component},
    window::WindowSize,
};
use std::io;

/// The integer wl_surface buffer scale. Fractional scaling is not represented here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scale(u32);

impl Scale {
    pub fn new(factor: i32) -> Result<Self, AppError> {
        if factor <= 0 {
            return Err(AppError::new(
                Component::Wayland,
                io::ErrorKind::InvalidInput,
            ));
        }
        Ok(Self(factor as u32))
    }

    pub fn factor(&self) -> u32 {
        self.0
    }

    pub fn to_physical(&self, size: WindowSize) -> Result<WindowSize, AppError> {
        let invalid = || AppError::new(Component::Wayland, io::ErrorKind::InvalidInput);
        WindowSize::new(
            size.width().checked_mul(self.0).ok_or_else(invalid)?,
            size.height().checked_mul(self.0).ok_or_else(invalid)?,
        )
    }

    /// Round outward so the logical extent covers every physical pixel.
    pub fn to_logical(&self, size: WindowSize) -> Result<WindowSize, AppError> {
        WindowSize::new(
            size.width().div_ceil(self.0),
            size.height().div_ceil(self.0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Scale;
    use crate::window::WindowSize;

    #[test]
    fn scale_roundtrip_pixel_bounds() {
        for factor in [1, 2, 3, 4] {
            let scale = Scale::new(factor).unwrap();
            let logical = WindowSize::new(801, 603).unwrap();
            let physical = scale.to_physical(logical).unwrap();
            assert_eq!(physical.width(), 801 * factor as u32);
            assert_eq!(physical.height(), 603 * factor as u32);
            assert_eq!(scale.to_logical(physical).unwrap(), logical);

            let physical = WindowSize::new(1001, 701).unwrap();
            let rounded = scale
                .to_physical(scale.to_logical(physical).unwrap())
                .unwrap();
            assert!(rounded.width() >= physical.width());
            assert!(rounded.height() >= physical.height());
            assert!(rounded.width() - physical.width() < factor as u32);
            assert!(rounded.height() - physical.height() < factor as u32);
        }
        assert!(Scale::new(0).is_err());
        assert!(Scale::new(-1).is_err());
        let limit = WindowSize::new(i32::MAX as u32, 1).unwrap();
        assert!(Scale::new(2).unwrap().to_physical(limit).is_err());
        let extreme = Scale::new(i32::MAX).unwrap();
        assert!(extreme.to_physical(WindowSize::new(3, 3).unwrap()).is_err());
        assert_eq!(
            extreme.to_logical(WindowSize::new(1, 1).unwrap()).unwrap(),
            WindowSize::new(1, 1).unwrap()
        );
    }
}
