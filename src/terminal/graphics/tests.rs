use crate::{TabId, kitty::graphics::store::ImageId, terminal::Terminal};
fn terminal() -> Terminal {
    Terminal::with_scrollback(TabId(1), 4, 8, 0).unwrap()
}
fn feed(term: &mut Terminal, bytes: &[u8]) {
    term.feed(term.tab(), bytes).unwrap();
}
#[test]
fn runtime_graphics_transfer_decodes_once_with_limits() {
    let mut first = terminal();
    let second = terminal();
    feed(&mut first, b"\x1b_Gi=7,f=32,s=1,v=1,m=1;AQID\x1b\\");
    assert_eq!(first.image_store().unwrap().used_bytes(), 0);
    feed(&mut first, b"\x1b_Gm=0;BA==\x1b\\");
    assert_eq!(
        first
            .image_store()
            .unwrap()
            .get(ImageId::Named(7))
            .unwrap()
            .pixels(),
        &[1, 2, 3, 4]
    );
    assert_eq!(second.image_store().unwrap().used_bytes(), 0);
    feed(&mut first, b"\x1b_Gi=8,f=32,s=1,v=1;!!!!\x1b\\");
    assert_eq!(first.image_store().unwrap().used_bytes(), 4);
}

#[test]
fn runtime_graphics_placement_uses_current_screen_geometry() {
    let mut term = terminal();
    term.set_cell_pixels((10, 20));
    feed(
        &mut term,
        b"\x1b[2;3H\x1b_Ga=T,i=1,p=2,f=32,s=1,v=1,c=2,r=1;AQIDBA==\x1b\\",
    );
    let p = &term.image_store().unwrap().placements()[0];
    assert_eq!(
        (p.rect.x, p.rect.y, p.rect.width, p.rect.height),
        (2, 1, 2, 1)
    );
    assert_eq!(term.grid().unwrap().cursor.point.column.0, 4);
    assert_eq!(term.grid().unwrap().cursor.point.line.0, 2);
    feed(&mut term, b"\x1b[?1049h\x1b[H\x1b_Ga=p,i=1,p=3,C=1\x1b\\");
    assert_eq!(
        term.image_store().unwrap().placements()[1].screen,
        crate::selection::Screen::Alternate
    );
    assert_eq!(term.grid().unwrap().cursor.point.column.0, 0);
    feed(
        &mut term,
        b"\x1b_Ga=T,i=2,f=32,s=2,v=1,x=1,w=20,X=2,Y=3,c=2,r=2,C=1;AQIDBAUGBwg=\x1b\\",
    );
    let p = term.image_store().unwrap().placements().last().unwrap();
    assert_eq!((p.source.x, p.source.width, p.source.height), (1, 1, 1));
    assert_eq!(p.offset, (2, 12));
    assert_eq!(p.pixels, (18, 18));
    assert_eq!((p.rect.width, p.rect.height), (2, 2));
    term.take_core_replies();
    feed(&mut term, b"\x1b_Ga=p,i=2,z=-2147483648\x1b\\");
    assert!(term.take_core_replies().0.starts_with(b"\x1b_Gi=2;ENOTSUP"));
}

#[test]
fn runtime_graphics_replies_follow_query_and_quiet_policy() {
    let mut term = terminal();
    feed(&mut term, b"\x1b_Ga=q,i=3,f=32,s=1,v=1;AQIDBA==\x1b\\");
    assert_eq!(term.take_core_replies().0, b"\x1b_Gi=3;OK\x1b\\");
    assert_eq!(term.image_store().unwrap().used_bytes(), 0);
    feed(&mut term, b"\x1b_Ga=t,i=3,q=1,f=32,s=1,v=1;AQIDBA==\x1b\\");
    assert!(term.take_core_replies().0.is_empty());
    feed(&mut term, b"\x1b_Gi=3,q=2,t=f;AAAA\x1b\\X");
    assert!(term.take_core_replies().0.is_empty());
    feed(&mut term, b"\x1b_Gi=4,t=f;AAAA\x1b\\");
    assert!(term.take_core_replies().0.starts_with(b"\x1b_Gi=4;EACCES"));
}

#[test]
fn runtime_graphics_lifecycle_reclaims_owned_resources() {
    let mut term = terminal();
    term.set_cell_pixels((10, 20));
    let image = b"\x1b_Ga=T,i=1,f=32,s=1,v=1,C=1;AQIDBA==\x1b\\";
    feed(&mut term, image);
    feed(&mut term, b"\x1b_Ga=d,d=I,i=1\x1b\\");
    assert_eq!(term.image_store().unwrap().used_bytes(), 0);
    feed(&mut term, image);
    feed(&mut term, b"\x1b[2J");
    assert!(term.image_store().unwrap().placements().is_empty());
    feed(&mut term, image);
    feed(&mut term, b"\x1bc");
    assert_eq!(term.image_store().unwrap().used_bytes(), 0);
    feed(&mut term, image);
    feed(&mut term, b"\x1b[4;1H\n");
    assert!(term.image_store().unwrap().placements().is_empty());
    feed(&mut term, b"\x1b[1;2r\x1b[H");
    feed(&mut term, image);
    term.resize(3, 8).unwrap();
    assert!(term.image_store().unwrap().placements().is_empty());
    feed(&mut term, image);
    feed(&mut term, b"\x1b[2;1H\n");
    assert_eq!(term.image_store().unwrap().placements().len(), 1);
    feed(&mut term, b"\n");
    assert!(term.image_store().unwrap().placements().is_empty());
    feed(&mut term, image);
    feed(&mut term, "\x1b[3;8H中".as_bytes());
    assert!(term.image_store().unwrap().placements().is_empty());
}

#[test]
fn runtime_graphics_flood_stays_within_global_budget() {
    use std::rc::Rc;
    let budget = Rc::new(super::Budget::new(1024 * 1024));
    let limits = crate::kitty::graphics::decode::ImageLimits::new(4096, 1024).unwrap();
    let mut a = terminal();
    let mut b = terminal();
    a.set_image_limits(limits).unwrap();
    b.set_image_limits(limits).unwrap();
    a.core
        .as_mut()
        .unwrap()
        .graphics
        .bind_budget(budget.clone())
        .unwrap();
    b.core
        .as_mut()
        .unwrap()
        .graphics
        .bind_budget(budget.clone())
        .unwrap();
    feed(&mut a, b"\x1b_Gi=1,f=32,s=1,v=1;AQIDBA==\x1b\\");
    budget.reserve(900_000).unwrap();
    feed(&mut b, b"\x1b_Gi=2,f=32,s=1,v=1;AQIDBA==\x1b\\");
    assert_eq!(b.image_store().unwrap().used_bytes(), 0);
    assert!(b.take_core_replies().0.starts_with(b"\x1b_Gi=2;E2BIG"));
    budget.used.set(budget.used.get() - 900_000);
    let before = budget.used.get();
    drop(a);
    assert!(budget.used.get() < before);
    drop(b);
    assert_eq!(budget.used.get(), 0);
    let mut term = terminal();
    let core = term.core.as_mut().unwrap();
    core.graphics.work = super::DECODE_WORK;
    for byte in b"\x1b_Gi=3,f=32,s=1,v=1;AQIDBA==\x1b\\" {
        core.graphics
            .push(*byte, &mut core.term, core.cell_pixels, &core.events)
            .unwrap();
    }
    assert_eq!(term.image_store().unwrap().used_bytes(), 0);
    assert!(term.take_core_replies().0.starts_with(b"\x1b_Gi=3;E2BIG"));
    feed(&mut term, b"\x1b_Gi=3,f=32,s=1,v=1;AQIDBA==\x1b\\");
    assert_eq!(term.image_store().unwrap().used_bytes(), 4);
    feed(&mut term, b"\x1bc");
    term.set_cell_pixels((10, 20));
    for id in 1..=65 {
        feed(
            &mut term,
            format!("\x1b_Gi={id},q=2,f=32,s=1,v=1;AQIDBA==\x1b\\").as_bytes(),
        );
    }
    assert_eq!(term.image_store().unwrap().used_bytes(), 64 * 4);
    assert!(
        term.image_store()
            .unwrap()
            .get(ImageId::Named(65))
            .is_none()
    );
    for id in 1..=257 {
        feed(
            &mut term,
            format!("\x1b_Ga=p,i=1,p={id},q=2,C=1\x1b\\").as_bytes(),
        );
    }
    assert_eq!(term.image_store().unwrap().placements().len(), 256);
    feed(&mut term, b"\x1b_Ga=d,d=I,i=1\x1b\\");
    assert!(term.image_store().unwrap().placements().is_empty());
    assert_eq!(term.image_store().unwrap().used_bytes(), 63 * 4);
}

#[test]
fn runtime_graphics_limits_png_cancellation_and_recovery() {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let mut term = terminal();
    term.set_image_limits(crate::kitty::graphics::decode::ImageLimits::new(4096, 1).unwrap())
        .unwrap();
    let png = include_bytes!("../../../tests/fixtures/pixel.png");
    feed(
        &mut term,
        format!("\x1b_Ga=t,i=9,f=100;{}\x1b\\", STANDARD.encode(png)).as_bytes(),
    );
    assert!(term.take_core_replies().0.ends_with(b";OK\x1b\\"));
    assert_eq!(term.image_store().unwrap().used_bytes(), 4);
    feed(
        &mut term,
        format!(
            "\x1b_Gi=8,f=100;{}\x1b\\",
            STANDARD.encode(include_bytes!(
                "../../../tests/fixtures/invalid-palette.png"
            ))
        )
        .as_bytes(),
    );
    assert!(term.take_core_replies().0.starts_with(b"\x1b_Gi=8;EINVAL"));
    assert_eq!(term.image_store().unwrap().used_bytes(), 4);
    feed(
        &mut term,
        b"\x1b_Gi=10,f=32,s=1,v=1,m=1;AQID\x1b\\\x18\x1b_Gm=0;BA==\x1b\\",
    );
    assert!(
        term.image_store()
            .unwrap()
            .get(ImageId::Named(10))
            .is_none()
    );
    feed(
        &mut term,
        b"\x1b_Gi=11,s=4294967295,v=4294967295;AAAA\x1b\\",
    );
    assert!(term.take_core_replies().0.starts_with(b"\x1b_Gi=11;E"));
    let overlong = format!("\x1b_Gi=12;{}\x1b\\Z", "A".repeat(4097));
    feed(&mut term, overlong.as_bytes());
    assert!(term.take_core_replies().0.starts_with(b"\x1b_Gi=12;E2BIG"));
    assert_eq!(
        term.grid().unwrap()[alacritty_terminal::index::Line(0)]
            [alacritty_terminal::index::Column(0)]
        .c,
        'Z'
    );
}
