//! Bounded object-to-offer identity mapping, independent of the native proxy type.
use crate::clipboard::{ClipboardOwner, ClipboardState, DataOffer};
const MAX_OFFERS: usize = 4;
pub(super) struct Offers<K> {
    pub state: ClipboardState,
    entries: Vec<(K, DataOffer)>,
}
impl<K> Default for Offers<K> {
    fn default() -> Self {
        Self {
            state: ClipboardState::new(),
            entries: Vec::new(),
        }
    }
}
impl<K: Eq> Offers<K> {
    pub fn new_primary() -> Self {
        Self {
            state: ClipboardState::new_primary(),
            entries: Vec::new(),
        }
    }
    pub fn created(&mut self, key: K) -> Result<(), K> {
        if self.entries.len() == MAX_OFFERS
            || self.entries.iter().any(|(old, _)| old == &key)
            || self.entries.try_reserve(1).is_err()
        {
            return Err(key);
        }
        let Some(id) = self.state.offer_created() else {
            return Err(key);
        };
        self.entries.push((key, DataOffer::new(id)));
        Ok(())
    }
    pub fn advertise(&mut self, key: &K, mime: &str) -> Result<(), ()> {
        let (_, offer) = self
            .entries
            .iter_mut()
            .find(|(old, _)| old == key)
            .ok_or(())?;
        if offer.advertise(mime).is_err() {
            // Keep this bounded identity until Selection retires the previous owner.
            offer.cancel();
            return Err(());
        }
        Ok(())
    }
    pub fn discard(&mut self, key: &K) -> Option<K> {
        let index = self.entries.iter().position(|(old, _)| old == key)?;
        let (key, offer) = self.entries.swap_remove(index);
        self.state.cancel_offer(offer.id());
        Some(key)
    }
    pub fn selected(&self) -> Option<(&K, &DataOffer)> {
        let ClipboardOwner::External(id) = self.state.owner()? else {
            return None;
        };
        self.entries
            .iter()
            .find(|(_, offer)| offer.id() == id)
            .map(|(key, offer)| (key, offer))
    }
    pub fn clear(&mut self) -> Vec<K> {
        self.state.clear_selection();
        self.clear_external()
    }
    pub fn clear_external(&mut self) -> Vec<K> {
        std::mem::take(&mut self.entries)
            .into_iter()
            .map(|(key, offer)| {
                self.state.cancel_offer(offer.id());
                key
            })
            .collect()
    }
    pub fn select(&mut self, key: Option<&K>) -> (bool, Vec<K>) {
        let Some(key) = key else {
            let changed = self.state.owner().is_some() || !self.entries.is_empty();
            return (changed, self.clear());
        };
        let Some((_, offer)) = self.entries.iter().find(|(old, _)| old == key) else {
            return (false, Vec::new());
        };
        let id = offer.id();
        let rejected = offer.is_cancelled();
        if self.state.owner() == Some(ClipboardOwner::External(id)) {
            return (false, Vec::new());
        }
        if !self.state.select_offer(id) {
            return (false, self.discard(key).into_iter().collect());
        }
        if rejected {
            return (true, self.clear());
        }
        let mut retired = Vec::new();
        let mut index = 0;
        while index < self.entries.len() {
            if self.entries[index].1.id() == id {
                index += 1;
                continue;
            }
            let (key, offer) = self.entries.swap_remove(index);
            self.state.cancel_offer(offer.id());
            retired.push(key);
        }
        (true, retired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_primary_selection_never_uses_clipboard_offer() {
        use super::super::clipboard_receive::Receive;
        use std::io::Cursor;
        let mut clipboard = Offers::default();
        clipboard.created(1).unwrap();
        clipboard.advertise(&1, "text/plain").unwrap();
        clipboard.select(Some(&1));
        let ordinary = clipboard.selected().unwrap().1.id();
        let mut primary = Offers::new_primary();
        assert!(primary.selected().is_none()); // No fallback, including missing protocol.
        primary.created(1).unwrap();
        primary.advertise(&1, "text/plain;charset=utf-8").unwrap();
        primary.select(Some(&1));
        let selected = primary.selected().unwrap().1.id();
        assert_ne!(selected, ordinary); // Even identical generations belong to distinct selections.
        let mut receive = Receive::default();
        let id = receive
            .start(selected, Cursor::new(b"primary"), 16)
            .unwrap();
        assert!(receive.pump(Some(id), &clipboard).is_err());
        let id = receive
            .start(selected, Cursor::new(b"primary"), 16)
            .unwrap();
        assert!(receive.pump(Some(id), &primary).unwrap().is_none());
        assert_eq!(
            receive.pump(Some(id), &primary).unwrap().as_deref(),
            Some("primary")
        );
        primary.clear();
        primary.state.set_local_owner(crate::TabId(7));
        assert_eq!(clipboard.selected().unwrap().1.id(), ordinary);
        assert!(primary.selected().is_none());
    }
    #[test]
    fn focus_loss_retires_external_but_preserves_local_owner() {
        let mut offers = Offers::default();
        offers.created(1).unwrap();
        offers.advertise(&1, "text/plain").unwrap();
        offers.select(Some(&1));
        assert_eq!(offers.clear_external(), [1]);
        assert!(offers.selected().is_none());
        assert!(!offers.select(Some(&1)).0);
        let tab = crate::TabId(3);
        offers.state.set_local_owner(tab);
        offers.created(2).unwrap();
        assert_eq!(offers.clear_external(), [2]);
        assert_eq!(offers.state.owner(), Some(ClipboardOwner::Local(tab)));
        offers.created(3).unwrap();
        assert!(offers.select(Some(&3)).0);
        offers.state.cancel_local_owner(tab);
        assert!(offers.selected().is_some());
    }
    #[test]
    fn rejected_replacement_retires_previous_clipboard() {
        for oversized_mime in [true, false] {
            let mut offers = Offers::default();
            offers.created(1).unwrap();
            offers.advertise(&1, "text/plain").unwrap();
            offers.select(Some(&1));
            offers.created(2).unwrap();
            if oversized_mime {
                assert!(offers.advertise(&2, &"x".repeat(257)).is_err());
            } else {
                for i in 0..64 {
                    offers.advertise(&2, &format!("application/{i}")).unwrap();
                }
                assert!(offers.advertise(&2, "text/plain").is_err());
            }
            assert!(offers.advertise(&2, "text/plain").is_err());
            let (changed, retired) = offers.select(Some(&2));
            assert!(changed);
            assert_eq!(retired.len(), 2);
            assert!(retired.contains(&1) && retired.contains(&2));
            assert!(offers.selected().is_none());
            assert!(offers.state.owner().is_none());
            assert!(!offers.select(Some(&1)).0);
            offers.created(3).unwrap();
            offers.advertise(&3, "text/plain").unwrap();
            assert!(offers.select(Some(&3)).0);
            assert!(offers.selected().is_some());
        }
    }
    #[test]
    fn runtime_clipboard_offers_ignore_stale_objects() {
        let mut offers = Offers::default();
        offers.created(1).unwrap();
        offers.advertise(&1, "text/plain").unwrap();
        offers.advertise(&1, "UTF8_STRING").unwrap();
        offers.advertise(&1, "text/plain;charset=utf-8").unwrap();
        assert!(offers.select(Some(&1)).0);
        assert_eq!(
            offers.selected().unwrap().1.preferred_text_mime(),
            Some("text/plain;charset=utf-8")
        );
        assert!(!offers.select(Some(&99)).0);
        assert!(!offers.select(Some(&1)).0);
        offers.created(2).unwrap();
        offers.advertise(&2, "image/png").unwrap();
        let (changed, retired) = offers.select(Some(&2));
        assert!(changed);
        assert_eq!(retired, [1]);
        assert!(offers.selected().unwrap().1.preferred_text_mime().is_none());
        assert!(offers.advertise(&1, "text/plain").is_err());
        assert!(!offers.select(Some(&1)).0);
        assert!(offers.select(None).0);
        assert!(offers.selected().is_none());
        offers.created(3).unwrap();
        offers.created(4).unwrap();
        assert_eq!(offers.select(Some(&3)), (false, vec![3]));
        assert!(offers.select(Some(&4)).0);
        for key in 5..8 {
            offers.created(key).unwrap();
        }
        assert_eq!(offers.created(8), Err(8));
        assert!(offers.advertise(&7, &"x".repeat(257)).is_err());
        assert_eq!(offers.discard(&7), Some(7));
        assert_eq!(offers.clear().len(), 3);
        assert!(offers.selected().is_none());
        offers.created(9).unwrap();
        offers.state.set_local_owner(crate::TabId(1));
        assert_eq!(offers.select(Some(&9)), (false, vec![9]));
        assert_eq!(
            offers.state.owner(),
            Some(crate::clipboard::ClipboardOwner::Local(crate::TabId(1)))
        );
    }
}
