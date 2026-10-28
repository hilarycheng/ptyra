use crate::window::WindowSize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    fn right(&self) -> i64 {
        i64::from(self.x) + i64::from(self.width)
    }
    fn bottom(&self) -> i64 {
        i64::from(self.y) + i64::from(self.height)
    }

    pub fn clipped(self, bounds: WindowSize) -> Option<Self> {
        let left = i64::from(self.x).max(0);
        let top = i64::from(self.y).max(0);
        let right = self.right().min(i64::from(bounds.width()));
        let bottom = self.bottom().min(i64::from(bounds.height()));
        if right <= left || bottom <= top {
            return None;
        }
        Some(Self {
            x: left as i32,
            y: top as i32,
            width: (right - left) as u32,
            height: (bottom - top) as u32,
        })
    }
}

/// One clipped bounding rectangle keeps damage storage constant under event floods.
pub struct Damage {
    bounds: WindowSize,
    pending: Option<Rect>,
}

impl Damage {
    pub fn new(bounds: WindowSize) -> Self {
        Self {
            bounds,
            pending: None,
        }
    }
    pub fn is_dirty(&self) -> bool {
        self.pending.is_some()
    }
    pub fn take(&mut self) -> Option<Rect> {
        self.pending.take()
    }

    pub fn add(&mut self, rect: Rect) {
        let Some(rect) = rect.clipped(self.bounds) else {
            return;
        };
        self.pending = Some(match self.pending {
            None => rect,
            Some(previous) => {
                let x = previous.x.min(rect.x);
                let y = previous.y.min(rect.y);
                Rect {
                    x,
                    y,
                    width: (previous.right().max(rect.right()) - i64::from(x)) as u32,
                    height: (previous.bottom().max(rect.bottom()) - i64::from(y)) as u32,
                }
            }
        });
    }

    pub fn resize(&mut self, bounds: WindowSize) {
        if bounds != self.bounds {
            self.bounds = bounds;
            self.pending = Some(Rect {
                x: 0,
                y: 0,
                width: bounds.width(),
                height: bounds.height(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Damage, Rect};
    use crate::window::WindowSize;

    #[test]
    fn damage_is_clipped_to_surface() {
        let mut damage = Damage::new(WindowSize::new(100, 80).unwrap());
        assert!(!damage.is_dirty());
        damage.add(Rect {
            x: -10,
            y: -5,
            width: 20,
            height: 30,
        });
        damage.add(Rect {
            x: 200,
            y: 200,
            width: 1,
            height: 1,
        });
        assert_eq!(
            damage.take(),
            Some(Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 25
            })
        );
        assert_eq!(damage.take(), None);
        damage.add(Rect {
            x: 10,
            y: 20,
            width: 0,
            height: 5,
        });
        assert!(!damage.is_dirty());
        damage.add(Rect {
            x: 10,
            y: 20,
            width: 20,
            height: 20,
        });
        damage.add(Rect {
            x: 90,
            y: 70,
            width: 30,
            height: 40,
        });
        assert_eq!(
            damage.take(),
            Some(Rect {
                x: 10,
                y: 20,
                width: 90,
                height: 60
            })
        );
        damage.add(Rect {
            x: i32::MIN,
            y: i32::MIN,
            width: u32::MAX,
            height: u32::MAX,
        });
        assert_eq!(
            damage.take(),
            Some(Rect {
                x: 0,
                y: 0,
                width: 100,
                height: 80
            })
        );
        damage.add(Rect {
            x: i32::MAX,
            y: i32::MAX,
            width: u32::MAX,
            height: u32::MAX,
        });
        assert!(!damage.is_dirty());
        damage.resize(WindowSize::new(50, 40).unwrap());
        assert_eq!(
            damage.take(),
            Some(Rect {
                x: 0,
                y: 0,
                width: 50,
                height: 40
            })
        );
        damage.resize(WindowSize::new(50, 40).unwrap());
        assert!(!damage.is_dirty());
    }
}
