use super::{ClipboardOwner, ClipboardState};
use crate::TabId;

#[test]
fn clipboard_state_rejects_stale_offer() {
    let mut clipboard = ClipboardState::new();
    assert_eq!(clipboard.owner(), None);
    let old = clipboard.offer_created().unwrap();
    let newer = clipboard.offer_created().unwrap();
    assert!(clipboard.select_offer(newer));
    assert_eq!(clipboard.owner(), Some(ClipboardOwner::External(newer)));
    assert!(!clipboard.select_offer(old));
    assert!(!clipboard.select_offer(newer));
    clipboard.cancel_offer(old);
    assert_eq!(clipboard.owner(), Some(ClipboardOwner::External(newer)));
    let pending = clipboard.offer_created().unwrap();
    assert_eq!(clipboard.owner(), Some(ClipboardOwner::External(newer)));
    clipboard.cancel_offer(pending);
    assert!(!clipboard.select_offer(pending));
    assert_eq!(clipboard.owner(), Some(ClipboardOwner::External(newer)));
    assert!(clipboard.set_local_owner(TabId(7)));
    assert_eq!(clipboard.owner(), Some(ClipboardOwner::Local(TabId(7))));
    assert!(!clipboard.select_offer(newer));
    clipboard.cancel_offer(newer);
    assert_eq!(clipboard.owner(), Some(ClipboardOwner::Local(TabId(7))));
    let newest = clipboard.offer_created().unwrap();
    assert!(clipboard.select_offer(newest));
    assert_eq!(clipboard.owner(), Some(ClipboardOwner::External(newest)));
    clipboard.cancel_offer(newest);
    assert_eq!(clipboard.owner(), None);
    assert!(!clipboard.select_offer(newest));
}
