use super::*;
use crate::{TabId, tabs::TabPty};
struct Fake(TabId);
impl TabPty for Fake {
    fn tab(&self) -> TabId {
        self.0
    }
}

#[test]
fn runtime_resource_caps_apply_to_actual_owners() {
    let config = Config::from_toml(Some("glyph_cache_bytes=2048\nscrollback_lines=0")).unwrap();
    let font =
        FontVec::try_from_vec(include_bytes!("../../../tests/fixtures/demo.ttf").to_vec()).unwrap();
    let mut raster = Raster::new(vec![font], &config).unwrap();
    let fonts = raster.fonts.as_ptr();
    let mut tabs = config.tab_registry::<Fake>().unwrap();
    let mut pixels = vec![0; 128 * 64 * 4];
    for id in 1..=8 {
        tabs.insert(TabId(id), 2, 8).unwrap();
        tabs.get_mut(TabId(id))
            .unwrap()
            .terminal_mut()
            .feed(TabId(id), b"ABCD")
            .unwrap();
        tabs.activate(TabId(id));
        raster
            .draw(
                tabs.get(TabId(id)).unwrap().terminal(),
                (128, 64),
                &mut pixels,
                false,
                true,
            )
            .unwrap();
        assert!(raster.cache.bytes_used() <= config.glyph_cache_bytes);
        assert!(!raster.cache.is_empty());
        assert_eq!(raster.fonts.as_ptr(), fonts);
    }
    let cached = raster.cache.bytes_used();
    tabs.close(TabId(8));
    assert_eq!(
        raster.cache.bytes_used(),
        cached,
        "font/glyph owners are shared by the Window"
    );
    raster.set_scale(2);
    assert_eq!(raster.cache.bytes_used(), 0);
    assert_eq!(raster.fonts.as_ptr(), fonts);
}
