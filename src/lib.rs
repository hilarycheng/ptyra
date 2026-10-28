pub const APP_NAME: &str = "Ptyra";

#[cfg(test)]
mod tests {
    #[test]
    fn workspace_smoke() {
        assert_eq!(crate::APP_NAME, "Ptyra");
    }
}
