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
    pub source: Rect,
    pub offset: (u32, u32),
    pub pixels: (u32, u32),
    pub z: i32,
    pub cell: (u32, u32),
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
    /// Return reclaimed pixel bytes; placements on another screen keep their image alive.
    pub fn delete(
        &mut self,
        header: &super::Header,
        screen: Screen,
        transfer: &mut super::transfer::Assembler,
    ) -> Result<usize, AppError> {
        if header.get(b'a') != Some(i64::from(b'd')) {
            return Err(invalid());
        }
        transfer.cancel();
        let mode = u8::try_from(header.value(b'd', i64::from(b'a'))).map_err(|_| invalid())?;
        let image = match mode {
            b'i' | b'I' => Some(ImageId::Named(
                u32::try_from(header.get(b'i').ok_or_else(invalid)?).map_err(|_| invalid())?,
            )),
            b'a' | b'A' => None,
            _ => return Err(invalid()),
        };
        let placement = header
            .get(b'p')
            .filter(|id| *id != 0)
            .map(|id| PlacementId::Named(id as u32));
        let selected = |p: &Placement| match image {
            Some(image) => p.image == image && placement.is_none_or(|id| p.id == id),
            None => p.screen == screen,
        };
        let mut affected = [None; MAX_IMAGES];
        for (index, (id, _)) in self.images.iter().enumerate() {
            if image == Some(*id) && placement.is_none()
                || self
                    .placements
                    .iter()
                    .any(|p| p.image == *id && selected(p))
            {
                affected[index] = Some(*id);
            }
        }
        self.placements.retain(|p| !selected(p));
        let before = self.used_bytes();
        if mode.is_ascii_uppercase() {
            self.images.retain(|(id, _)| {
                !affected.contains(&Some(*id)) || self.placements.iter().any(|p| p.image == *id)
            });
        }
        Ok(before - self.used_bytes())
    }

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
    pub(crate) fn clear_screen(&mut self, screen: Screen) -> bool {
        let affected: Vec<_> = self
            .placements
            .iter()
            .filter(|p| p.screen == screen)
            .map(|p| p.image)
            .collect();
        self.placements.retain(|p| p.screen != screen);
        let changed = !affected.is_empty();
        self.images.retain(|(id, _)| {
            !affected.contains(id) || self.placements.iter().any(|p| p.image == *id)
        });
        changed
    }
    pub fn tab(&self) -> TabId {
        self.tab
    }
    pub(crate) fn allocated_bytes(&self) -> usize {
        self.images
            .iter()
            .map(|(_, i)| i.rgba.capacity())
            .sum::<usize>()
            + self.images.capacity() * std::mem::size_of::<(ImageId, Image)>()
            + self.placements.capacity() * std::mem::size_of::<Placement>()
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
    pub(crate) fn placements_mut(&mut self) -> &mut Vec<Placement> {
        &mut self.placements
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
        let source = self.get(image).ok_or_else(invalid)?;
        let placement = Placement {
            source: Rect {
                x: 0,
                y: 0,
                width: source.width,
                height: source.height,
            },
            offset: (0, 0),
            pixels: (source.width, source.height),
            z: 0,
            cell: (1, 1),
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
    #[test]
    fn kitty_delete_releases_memory() {
        use crate::kitty::graphics::{GraphicsCommand, transfer::Assembler};
        let mut store = ImageStore::new(TabId(1), ImageLimits::default(), 8).unwrap();
        let first = store.put(image(Some(1), 1)).unwrap();
        let second = store.put(image(Some(2), 2)).unwrap();
        assert!(store.put(image(Some(3), 3)).is_err());
        let rect = Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
        store.place(first, Some(7), Screen::Main, rect).unwrap();
        store
            .place(first, Some(8), Screen::Alternate, rect)
            .unwrap();
        store.place(second, Some(7), Screen::Main, rect).unwrap();
        let mut transfer = Assembler::default();
        transfer
            .push(GraphicsCommand {
                header: Header::parse(b"m=1").unwrap(),
                payload: b"AAAA".to_vec(),
            })
            .unwrap();
        assert_eq!(
            store
                .delete(
                    &Header::parse(b"a=d,d=I,i=1,p=7").unwrap(),
                    Screen::Main,
                    &mut transfer
                )
                .unwrap(),
            0
        );
        assert_eq!(store.used_bytes(), 8);
        assert_eq!(
            store
                .delete(
                    &Header::parse(b"a=d,d=I,i=1").unwrap(),
                    Screen::Main,
                    &mut transfer
                )
                .unwrap(),
            4
        );
        store.put(image(Some(3), 3)).unwrap();
        assert_eq!(
            store
                .delete(
                    &Header::parse(b"a=d,d=i,i=2").unwrap(),
                    Screen::Main,
                    &mut transfer
                )
                .unwrap(),
            0
        );
        assert!(store.get(second).is_some());
        assert_eq!(
            store
                .delete(
                    &Header::parse(b"a=d,d=I,i=2").unwrap(),
                    Screen::Main,
                    &mut transfer
                )
                .unwrap(),
            4
        );
        let complete = transfer
            .push(GraphicsCommand {
                header: Header::parse(b"s=1,v=1").unwrap(),
                payload: b"AQIDBA==".to_vec(),
            })
            .unwrap()
            .unwrap();
        assert_eq!(complete.bytes, [1, 2, 3, 4]);
        assert_eq!(store.used_bytes(), 4);
    }
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
