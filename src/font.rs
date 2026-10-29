//! Pure font metrics and fallback selection; no font loading, hinting or platform I/O.
use crate::error::{AppError, Component};
use std::io::ErrorKind;

/// Returns the first supporting candidate's index; put the primary font first.
/// `has_glyph` queries loaded Unicode coverage without I/O and must return false
/// for missing-glyph placeholders. Empty lists or entirely missing coverage yield None.
/// This selects one scalar only; sequences and shaping belong to the font backend.
pub fn select_font<F>(
    character: char,
    candidates: &[F],
    mut has_glyph: impl FnMut(&F, char) -> bool,
) -> Option<usize> {
    candidates
        .iter()
        .position(|candidate| has_glyph(candidate, character))
}

/// Caller-supplied metrics for a monospace face, normalized to em units.
/// Advance is the fixed horizontal pen advance, not the glyph bounding-box width.
#[derive(Debug, Clone, Copy)]
pub struct FontMetrics {
    pub advance: f64,
    /// Nonnegative distance above the baseline.
    pub ascent: f64,
    /// Nonpositive coordinate below the baseline.
    pub descent: f64,
    pub line_gap: f64,
}

/// Unrounded logical-pixel dimensions; physical scaling belongs to CellLayout.
#[derive(Debug, Clone, Copy)]
pub struct CellMetrics {
    width: f64,
    height: f64,
    baseline: f64,
}

impl CellMetrics {
    /// `font_size` is logical pixels per em, not points or physical pixels.
    /// Size and resulting advances must be positive and at most i32::MAX.
    /// Line gap must be nonnegative and follows descent, below the baseline.
    pub fn new(font_size: f64, font: FontMetrics) -> Result<Self, AppError> {
        let invalid = || AppError::new(Component::Renderer, ErrorKind::InvalidInput);
        let valid_dimension = |value: f64| value > 0.0 && value <= f64::from(i32::MAX);
        if !valid_dimension(font_size)
            || ![font.advance, font.ascent, font.descent, font.line_gap]
                .into_iter()
                .all(f64::is_finite)
            || font.advance <= 0.0
            || font.ascent < 0.0
            || font.descent > 0.0
            || font.line_gap < 0.0
            || (font.ascent == 0.0 && font.descent == 0.0)
        {
            return Err(invalid());
        }
        let width = font_size * font.advance;
        let baseline = font_size * font.ascent;
        // Scale first so a large em sum cannot overflow before a small size applies.
        let height = baseline - font_size * font.descent + font_size * font.line_gap;
        if !valid_dimension(width) || !valid_dimension(height) {
            return Err(invalid());
        }
        Ok(Self {
            width,
            height,
            baseline,
        })
    }

    pub fn width(&self) -> f64 {
        self.width
    }

    pub fn height(&self) -> f64 {
        self.height
    }

    /// Distance from the cell's top edge to its baseline, in logical pixels.
    pub fn baseline(&self) -> f64 {
        self.baseline
    }
}

#[cfg(test)]
mod tests {
    use super::{CellMetrics, FontMetrics, select_font};
    use crate::error::{AppError, Component};
    use std::io::ErrorKind;

    const FACE: FontMetrics = FontMetrics {
        advance: 0.5,
        ascent: 0.75,
        descent: -0.25,
        line_gap: 0.125,
    };

    #[test]
    fn font_fallback_uses_cjk_candidate() {
        let fonts = [
            ("primary", " A"),
            ("CJK label alone is insufficient", "漢"),
            ("traditional coverage", "A繁體中文𠮷"),
            ("later duplicate coverage", "A繁體中文𠮷"),
        ];
        for character in "繁體中文𠮷".chars() {
            let mut probes = Vec::new();
            let selected = select_font(character, &fonts, |font, scalar| {
                assert_eq!(scalar, character);
                probes.push(font.0);
                font.1.contains(scalar)
            });
            assert_eq!(selected, Some(2));
            assert_eq!(probes, [fonts[0].0, fonts[1].0, fonts[2].0]);
        }
        let mut probes = Vec::new();
        assert_eq!(
            select_font('A', &fonts, |font, scalar| {
                probes.push(font.0);
                font.1.contains(scalar)
            }),
            Some(0)
        );
        assert_eq!(probes, [fonts[0].0]);
        let reordered = [fonts[3], fonts[2], fonts[0]];
        assert_eq!(
            select_font('繁', &reordered, |font, scalar| font.1.contains(scalar)),
            Some(0)
        );
    }

    #[test]
    fn font_fallback_reports_missing_and_checks_each_candidate_once() {
        let mut probes = 0;
        assert_eq!(
            select_font('繁', &[] as &[&str], |_, _| {
                probes += 1;
                false
            }),
            None
        );
        assert_eq!(probes, 0);

        let fonts = [" A", "繁體中文", "Ω"];
        assert_eq!(
            select_font('𠮷', &fonts, |font, scalar| {
                probes += 1;
                font.contains(scalar)
            }),
            None
        );
        assert_eq!(probes, fonts.len());
        // A missing result must not become a cached choice for the next character.
        assert_eq!(
            select_font('繁', &fonts, |font, scalar| font.contains(scalar)),
            Some(1)
        );
        assert_eq!(
            select_font('A', &fonts, |font, scalar| font.contains(scalar)),
            Some(0)
        );
    }

    #[test]
    fn font_metrics_validate_dimensions() {
        for (size, width, height, baseline) in [
            (12.0, 6.0, 13.5, 9.0),
            (13.0, 6.5, 14.625, 9.75),
            (13.5, 6.75, 15.1875, 10.125),
            (24.0, 12.0, 27.0, 18.0),
        ] {
            let metrics = CellMetrics::new(size, FACE).unwrap();
            assert_eq!(metrics.width(), width);
            assert_eq!(metrics.height(), height);
            assert_eq!(metrics.baseline(), baseline);
        }
        let error = AppError::new(Component::Renderer, ErrorKind::InvalidInput);
        for size in [
            0.0,
            -0.0,
            -1.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MAX,
            f64::from(i32::MAX) + 1.0,
            f64::from_bits(1),
        ] {
            assert_eq!(CellMetrics::new(size, FACE).unwrap_err(), error);
        }
        let tiny = CellMetrics::new(0.5, FACE).unwrap();
        assert_eq!(tiny.width(), 0.25);
        assert_eq!(tiny.height(), 0.5625);
        assert_eq!(tiny.baseline(), 0.375);
    }

    #[test]
    fn font_metrics_scale_before_summing_large_extents() {
        let metrics = CellMetrics::new(
            1e-307,
            FontMetrics {
                advance: 1e308,
                ascent: 1e308,
                descent: -1e308,
                line_gap: 0.0,
            },
        )
        .unwrap();
        assert!((metrics.width() - 10.0).abs() < 1e-12);
        assert!((metrics.height() - 20.0).abs() < 1e-12);
        assert!((metrics.baseline() - 10.0).abs() < 1e-12);
    }

    #[test]
    fn font_metrics_reject_invalid_face_metrics_and_overflow() {
        let error = AppError::new(Component::Renderer, ErrorKind::InvalidInput);
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for face in [
                FontMetrics {
                    advance: value,
                    ..FACE
                },
                FontMetrics {
                    ascent: value,
                    ..FACE
                },
                FontMetrics {
                    descent: value,
                    ..FACE
                },
                FontMetrics {
                    line_gap: value,
                    ..FACE
                },
            ] {
                assert_eq!(CellMetrics::new(12.0, face).unwrap_err(), error);
            }
        }
        for face in [
            FontMetrics {
                advance: 0.0,
                ..FACE
            },
            FontMetrics {
                advance: -0.5,
                ..FACE
            },
            FontMetrics {
                ascent: -0.75,
                ..FACE
            },
            FontMetrics {
                descent: 0.25,
                ..FACE
            },
            FontMetrics {
                line_gap: -0.125,
                ..FACE
            },
            FontMetrics {
                ascent: 0.0,
                descent: 0.0,
                ..FACE
            },
            FontMetrics {
                advance: f64::MAX,
                ..FACE
            },
            FontMetrics {
                ascent: f64::MAX,
                descent: -f64::MAX,
                ..FACE
            },
            FontMetrics {
                line_gap: f64::MAX,
                ..FACE
            },
            FontMetrics {
                advance: f64::from(i32::MAX),
                ..FACE
            },
        ] {
            assert_eq!(CellMetrics::new(12.0, face).unwrap_err(), error);
        }
        assert_eq!(
            CellMetrics::new(f64::from(i32::MAX), FACE).unwrap_err(),
            error
        );

        let maximum = CellMetrics::new(
            f64::from(i32::MAX),
            FontMetrics {
                advance: 1.0,
                ascent: 1.0,
                descent: 0.0,
                line_gap: 0.0,
            },
        )
        .unwrap();
        assert_eq!(maximum.width(), f64::from(i32::MAX));
        assert_eq!(maximum.height(), f64::from(i32::MAX));
        assert_eq!(maximum.baseline(), f64::from(i32::MAX));
        let below_baseline = CellMetrics::new(
            12.0,
            FontMetrics {
                ascent: 0.0,
                descent: -1.0,
                line_gap: 0.0,
                ..FACE
            },
        )
        .unwrap();
        assert_eq!(below_baseline.height(), 12.0);
        assert_eq!(below_baseline.baseline(), 0.0);
    }
}
