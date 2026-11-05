use super::{
    decode::{Image, ImageLimits},
    invalid,
};
use crate::{
    TabId,
    damage::Rect,
    error::{AppError, Component},
    selection::Screen,
};
use std::io::ErrorKind;

pub const MAX_STORE_BYTES: usize = 64 * 1024 * 1024;
const MAX_IMAGES: usize = 64;
const MAX_PLACEMENTS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageId {
    Named(u32),
    Anonymous(u64),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacementId {
    Named(u32),
    Anonymous(u64),
}
pub struct Placement {
    pub image: ImageId,
    pub id: PlacementId,
    pub screen: Screen,
    /// Cell coordinates, validated before conversion to renderer geometry.
    pub rect: Rect,
}
pub struct ImageStore {
    tab: TabId,
    limits: ImageLimits,
    budget: usize,
    images: Vec<(ImageId, Image)>,
    placements: Vec<Placement>,
    next_anonymous: u64,
}
impl ImageStore {
    pub fn new(tab: TabId, limits: ImageLimits, budget: usize) -> Result<Self, AppError> {
        if budget == 0 || budget > MAX_STORE_BYTES {
            return Err(invalid());
        }
        Ok(Self {
            tab,
            limits,
            budget,
            images: Vec::new(),
            placements: Vec::new(),
            next_anonymous: 1,
        })
    }
    pub fn tab(&self) -> TabId {
        self.tab
    }
    pub fn used_bytes(&self) -> usize {
        self.images.iter().map(|(_, image)| image.rgba.len()).sum()
    }
    pub fn get(&self, id: ImageId) -> Option<&Image> {
        self.images
            .iter()
            .find(|(key, _)| *key == id)
            .map(|(_, image)| image)
    }
    pub fn placements(&self) -> &[Placement] {
        &self.placements
    }
    pub fn put(&mut self, image: Image) -> Result<ImageId, AppError> {
        if !(*b"tT")
            .map(i64::from)
            .contains(&image.header.value(b'a', i64::from(b't')))
        {
            return Err(invalid());
        }
        let size = self.limits.check_dimensions(image.width, image.height)?;
        if size != image.rgba.len() {
            return Err(invalid());
        }
        let id = match image.header.get(b'i') {
            Some(id) => ImageId::Named(u32::try_from(id).map_err(|_| invalid())?),
            None => ImageId::Anonymous(self.next_anonymous),
        };
        let next = self.next_anonymous.checked_add(1).ok_or_else(invalid)?;
        let existing = self.images.iter().position(|(key, _)| *key == id);
        let previous = existing.map_or(0, |index| self.images[index].1.rgba.len());
        if size > self.budget - (self.used_bytes() - previous)
            || existing.is_none() && self.images.len() == MAX_IMAGES
        {
            return Err(AppError::new(Component::Terminal, ErrorKind::OutOfMemory));
        }
        if let Some(index) = existing {
            self.images[index].1 = image;
            self.placements.retain(|placement| placement.image != id);
        } else {
            self.images
                .try_reserve(1)
                .map_err(|_| AppError::new(Component::Terminal, ErrorKind::OutOfMemory))?;
            self.images.push((id, image));
        }
        if matches!(id, ImageId::Anonymous(_)) {
            self.next_anonymous = next;
        }
        Ok(id)
    }
    pub fn place(
        &mut self,
        image: ImageId,
        id: Option<u32>,
        screen: Screen,
        rect: Rect,
    ) -> Result<PlacementId, AppError> {
        if self.get(image).is_none() {
            return Err(AppError::new(Component::Terminal, ErrorKind::NotFound));
        }
        if rect.x < 0
            || rect.y < 0
            || rect.width == 0
            || rect.height == 0
            || rect.x as u64 + u64::from(rect.width) > u64::from(u16::MAX)
            || rect.y as u64 + u64::from(rect.height) > u64::from(u16::MAX)
        {
            return Err(invalid());
        }
        let id = match id.filter(|id| *id != 0) {
            Some(id) => PlacementId::Named(id),
            None => PlacementId::Anonymous(self.next_anonymous),
        };
        let next = self.next_anonymous.checked_add(1).ok_or_else(invalid)?;
        let existing = self
            .placements
            .iter()
            .position(|placement| placement.image == image && placement.id == id);
        let placement = Placement {
            image,
            id,
            screen,
            rect,
        };
        if let Some(index) = existing {
            self.placements[index] = placement;
        } else {
            if self.placements.len() == MAX_PLACEMENTS {
                return Err(AppError::new(Component::Terminal, ErrorKind::OutOfMemory));
            }
            self.placements
                .try_reserve(1)
                .map_err(|_| AppError::new(Component::Terminal, ErrorKind::OutOfMemory))?;
            self.placements.push(placement);
        }
        if matches!(id, PlacementId::Anonymous(_)) {
            self.next_anonymous = next;
        }
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kitty::graphics::Header;
    fn image(id: Option<u32>, color: u8) -> Image {
        let header = id.map_or_else(Header::default, |id| {
            Header::parse(format!("i={id}").as_bytes()).unwrap()
        });
        Image {
            header,
            width: 1,
            height: 1,
            rgba: vec![color; 4],
        }
    }
    #[test]
    fn kitty_image_ids_do_not_collide() {
        let mut store = ImageStore::new(TabId(1), ImageLimits::default(), 16).unwrap();
        let anonymous = store.put(image(None, 1)).unwrap();
        let first = store.put(image(Some(1), 2)).unwrap();
        let second = store.put(image(Some(2), 3)).unwrap();
        assert_ne!(anonymous, first);
        let rect = Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
        store.place(first, Some(7), Screen::Main, rect).unwrap();
        store.place(second, Some(7), Screen::Main, rect).unwrap();
        store
            .place(second, Some(7), Screen::Main, Rect { x: 1, ..rect })
            .unwrap();
        assert_eq!(store.placements().len(), 2);
        store.put(image(Some(1), 9)).unwrap();
        assert_eq!(store.placements().len(), 1);
        assert_eq!(store.get(second).unwrap().pixels(), [3; 4]);
        assert_eq!(store.get(anonymous).unwrap().pixels(), [1; 4]);
        store.place(second, None, Screen::Alternate, rect).unwrap();
        store.place(second, None, Screen::Alternate, rect).unwrap();
        assert_eq!(store.placements().len(), 3);
        let other = ImageStore::new(TabId(2), ImageLimits::default(), 16).unwrap();
        assert!(other.get(first).is_none());
        assert_eq!(store.used_bytes(), 12);
    }
}
