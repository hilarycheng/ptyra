pub const APP_NAME: &str = "Ptyra";

/// A window ID cannot be used where a tab ID is required.
///
/// ```compile_fail
/// use ptyra::{TabId, WindowId};
/// let _: TabId = WindowId(1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowId(pub u64);

#[cfg(test)]
mod tests {
    use super::{TabId, WindowId};
    use std::any::TypeId;

    #[test]
    fn workspace_smoke() {
        assert_eq!(crate::APP_NAME, "Ptyra");
    }

    #[test]
    fn typed_ids_are_distinct() {
        assert_ne!(TypeId::of::<TabId>(), TypeId::of::<WindowId>());
        assert_eq!(TabId(1), TabId(1));
        assert_ne!(TabId(1), TabId(2));
        assert_eq!(WindowId(1), WindowId(1));
        assert_ne!(WindowId(1), WindowId(2));
    }
}
