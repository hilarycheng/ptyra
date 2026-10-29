//! Owned glyph payloads with byte and entry budgets; no rasterizer or platform I/O.
use crate::error::{AppError, Component};
use std::{
    collections::{HashMap, VecDeque},
    io::ErrorKind,
};

pub const MAX_CACHE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_CACHE_ENTRIES: usize = 4096;

/// One immutable rasterization instance: change identity for face, size, scale,
/// style, hinting or format changes. A fallback-list index alone is not an identity.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub font_instance: u64,
    pub glyph: u32,
}

/// One cache can serve all Tabs. Keys and glyph payloads deliberately have no Debug.
pub struct GlyphCache {
    entries: HashMap<GlyphKey, Box<[u8]>>,
    order: VecDeque<GlyphKey>,
    used_bytes: usize,
    max_bytes: usize,
    max_entries: usize,
}

impl GlyphCache {
    /// Reserves metadata for the entry limit up front.
    /// Either zero limit disables caching. Payload accounting excludes allocator overhead.
    pub fn new(max_bytes: usize, max_entries: usize) -> Result<Self, AppError> {
        if max_bytes > MAX_CACHE_BYTES || max_entries > MAX_CACHE_ENTRIES {
            return Err(AppError::new(Component::Renderer, ErrorKind::InvalidInput));
        }
        let capacity = if max_bytes == 0 { 0 } else { max_entries };
        let out_of_memory = |_| AppError::new(Component::Renderer, ErrorKind::OutOfMemory);
        let mut entries = HashMap::new();
        entries.try_reserve(capacity).map_err(out_of_memory)?;
        let mut order = VecDeque::new();
        order.try_reserve_exact(capacity).map_err(out_of_memory)?;
        Ok(Self {
            entries,
            order,
            used_bytes: 0,
            max_bytes,
            max_entries,
        })
    }

    /// FIFO reads do not change eviction order or allocate recency records.
    pub fn get(&self, key: GlyphKey) -> Option<&[u8]> {
        self.entries.get(&key).map(|payload| payload.as_ref())
    }

    /// None means cached; Some returns the original payload when disabled, too large,
    /// or unable to reserve metadata. Rejected payloads can still be used uncached.
    /// Rejection preserves all entries and ordering. Replacement becomes newest.
    /// The caller owns payload allocation and format validation before this call.
    pub fn insert(&mut self, key: GlyphKey, payload: Box<[u8]>) -> Option<Box<[u8]>> {
        if self.max_entries == 0 || self.max_bytes == 0 || payload.len() > self.max_bytes {
            return Some(payload);
        }
        // HashMap tombstones can consume spare capacity; reserve before any eviction.
        if self.entries.try_reserve(1).is_err() {
            return Some(payload);
        }
        if let Some(previous) = self.entries.remove(&key) {
            self.used_bytes -= previous.len();
            self.order.retain(|old| *old != key);
        }
        // shortcut: FIFO can evict hot glyphs; add LRU only if measured misses justify it.
        while let Some(oldest) = self.order.front().copied() {
            if self.entries.len() < self.max_entries
                && self.used_bytes <= self.max_bytes - payload.len()
            {
                break;
            }
            self.order.pop_front();
            if let Some(previous) = self.entries.remove(&oldest) {
                self.used_bytes -= previous.len();
            }
        }
        self.used_bytes += payload.len();
        self.entries.insert(key, payload);
        self.order.push_back(key);
        None
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn bytes_used(&self) -> usize {
        self.used_bytes
    }

    /// Drops glyph payloads and retains metadata storage for reuse.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.used_bytes = 0;
    }
}

#[cfg(test)]
mod tests;
