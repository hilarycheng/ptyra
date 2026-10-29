use super::{GlyphCache, GlyphKey, MAX_CACHE_BYTES, MAX_CACHE_ENTRIES};
use crate::error::{AppError, Component};
use std::io::ErrorKind;

fn key(glyph: u32) -> GlyphKey {
    GlyphKey {
        font_instance: 1,
        glyph,
    }
}

fn insert(cache: &mut GlyphCache, glyph: u32, data: &[u8]) {
    assert!(cache.insert(key(glyph), data.into()).is_none());
}

#[test]
fn glyph_cache_evicts_by_limit() {
    let mut cache = GlyphCache::new(6, 3).unwrap();
    insert(&mut cache, 1, b"aa");
    insert(&mut cache, 2, b"bbb");
    insert(&mut cache, 3, b"ccc");
    assert_eq!(cache.bytes_used(), 6);
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.get(key(1)), None);
    assert_eq!(cache.get(key(2)), Some(&b"bbb"[..]));
    assert_eq!(cache.get(key(3)), Some(&b"ccc"[..]));
    // Reading an older entry does not change the documented FIFO order.
    insert(&mut cache, 4, b"ddd");
    assert_eq!(cache.get(key(2)), None);
    assert_eq!(cache.get(key(3)), Some(&b"ccc"[..]));
    assert_eq!(cache.get(key(4)), Some(&b"ddd"[..]));

    let mut cache = GlyphCache::new(6, 3).unwrap();
    for glyph in 1..=3 {
        insert(&mut cache, glyph, b"xx");
    }
    insert(&mut cache, 4, b"large");
    for glyph in 1..=3 {
        assert_eq!(cache.get(key(glyph)), None);
    }
    assert_eq!(cache.get(key(4)), Some(&b"large"[..]));
    assert_eq!(cache.bytes_used(), 5);
    assert_eq!(cache.len(), 1);
}

#[test]
fn glyph_cache_counts_empty_entries_and_rejects_when_disabled() {
    let mut cache = GlyphCache::new(6, 2).unwrap();
    for glyph in 1..=3 {
        insert(&mut cache, glyph, b"");
    }
    assert_eq!(cache.bytes_used(), 0);
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.get(key(1)), None);
    assert_eq!(cache.get(key(2)), Some(&b""[..]));
    assert_eq!(cache.get(key(3)), Some(&b""[..]));

    for (bytes, entries) in [(0, 0), (0, 2), (6, 0)] {
        let mut disabled = GlyphCache::new(bytes, entries).unwrap();
        assert_eq!(
            disabled.insert(key(1), b"".to_vec().into_boxed_slice()),
            Some(Box::from(&b""[..]))
        );
        assert!(disabled.is_empty());
        assert_eq!(disabled.bytes_used(), 0);
    }
}

#[test]
fn glyph_cache_replacement_updates_cost_and_order_once() {
    let mut cache = GlyphCache::new(8, 3).unwrap();
    insert(&mut cache, 1, b"aa");
    insert(&mut cache, 2, b"bbb");
    insert(&mut cache, 1, b"AAAA");
    assert_eq!(cache.bytes_used(), 7);
    assert_eq!(cache.len(), 2);
    insert(&mut cache, 3, b"ccc");
    assert_eq!(cache.get(key(2)), None);
    assert_eq!(cache.get(key(1)), Some(&b"AAAA"[..]));
    insert(&mut cache, 1, b"a");
    assert_eq!(cache.bytes_used(), 4);
    for _ in 0..512 {
        insert(&mut cache, 1, b"a");
        assert_eq!(cache.get(key(1)), Some(&b"a"[..]));
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.order.len(), 2);
        assert_eq!(cache.bytes_used(), 4);
    }
    insert(&mut cache, 4, b"dddd");
    insert(&mut cache, 5, b"");
    assert_eq!(cache.get(key(3)), None);
    assert_eq!(cache.len(), 3);
    assert_eq!(cache.bytes_used(), 5);
}

#[test]
fn glyph_cache_rejects_oversized_payload_without_eviction() {
    let mut cache = GlyphCache::new(6, 2).unwrap();
    insert(&mut cache, 1, b"aa");
    insert(&mut cache, 2, b"bbb");
    for glyph in [1, 3] {
        let payload: Box<[u8]> = Box::from(&b"too big"[..]);
        let original = payload.as_ptr();
        let rejected = cache.insert(key(glyph), payload).unwrap();
        assert_eq!(rejected.as_ptr(), original);
        assert_eq!(&*rejected, b"too big");
        assert_eq!(cache.bytes_used(), 5);
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(key(1)), Some(&b"aa"[..]));
        assert_eq!(cache.get(key(2)), Some(&b"bbb"[..]));
    }
    insert(&mut cache, 3, b"ccc");
    assert_eq!(cache.get(key(1)), None);
    assert_eq!(cache.get(key(2)), Some(&b"bbb"[..]));
    assert_eq!(cache.bytes_used(), 6);
}

#[test]
fn glyph_cache_isolates_font_instances_and_clear_reclaims_budget() {
    let mut cache = GlyphCache::new(6, 3).unwrap();
    for font_instance in 1..=3 {
        let payload = vec![font_instance as u8; 2].into_boxed_slice();
        assert!(
            cache
                .insert(
                    GlyphKey {
                        font_instance,
                        glyph: 42
                    },
                    payload
                )
                .is_none()
        );
    }
    for font_instance in 1..=3 {
        assert_eq!(
            cache.get(GlyphKey {
                font_instance,
                glyph: 42
            }),
            Some(&[font_instance as u8; 2][..])
        );
    }
    cache.clear();
    assert!(cache.is_empty());
    assert_eq!(cache.bytes_used(), 0);
    assert_eq!(
        cache.get(GlyphKey {
            font_instance: 1,
            glyph: 42
        }),
        None
    );
    insert(&mut cache, 4, b"123456");
    assert_eq!(cache.bytes_used(), 6);
    assert_eq!(cache.len(), 1);
}

#[test]
fn glyph_cache_validates_limits_before_reserving() {
    let error = AppError::new(Component::Renderer, ErrorKind::InvalidInput);
    for (bytes, entries) in [
        (MAX_CACHE_BYTES + 1, 1),
        (usize::MAX, 1),
        (1, MAX_CACHE_ENTRIES + 1),
        (1, usize::MAX),
    ] {
        assert_eq!(GlyphCache::new(bytes, entries).err(), Some(error));
    }
    let cache = GlyphCache::new(MAX_CACHE_BYTES, MAX_CACHE_ENTRIES).unwrap();
    assert!(cache.is_empty());
    assert_eq!(cache.bytes_used(), 0);
}

#[test]
fn glyph_cache_churn_keeps_payload_and_metadata_bounded() {
    let mut cache = GlyphCache::new(12, 3).unwrap();
    for glyph in 0u32..10_000 {
        let bytes = glyph.to_le_bytes();
        insert(&mut cache, glyph, &bytes);
        assert_eq!(cache.get(key(glyph)), Some(&bytes[..]));
        assert!(cache.bytes_used() <= 12);
        assert!(cache.len() <= 3);
        assert_eq!(cache.order.len(), cache.len());
        if glyph >= 3 {
            assert_eq!(cache.get(key(glyph - 3)), None);
        }
    }
}
