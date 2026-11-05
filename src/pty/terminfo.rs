use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Keep the entry available to child programs without installing global files.
pub(super) struct Terminfo(PathBuf);

impl Terminfo {
    pub fn new() -> io::Result<Self> {
        let entry = Self(nix::unistd::mkdtemp("/tmp/ptyra-terminfo-XXXXXX")?);
        fs::create_dir(entry.0.join("p"))?;
        fs::write(
            entry.0.join("p/ptyra"),
            include_bytes!("../../assets/ptyra.terminfo.bin"),
        )?;
        Ok(entry)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Terminfo {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0.join("p/ptyra"));
        let _ = fs::remove_dir(self.0.join("p"));
        let _ = fs::remove_dir(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn terminfo_is_private_and_removed_on_drop() {
        let entry = Terminfo::new().unwrap();
        let path = entry.path().to_owned();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::read(path.join("p/ptyra")).unwrap(),
            include_bytes!("../../assets/ptyra.terminfo.bin")
        );
        drop(entry);
        assert!(!path.exists());
    }
}
