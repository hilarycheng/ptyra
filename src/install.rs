//! Install plans and an exclusive Linux filesystem adapter.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod adapter;
use crate::error::{AppError, Component};
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub use adapter::{InstallFailure, InstallReceipt, InstalledFile, RollbackFailure};
use std::{
    io::ErrorKind,
    os::unix::ffi::OsStrExt,
    path::{Component as PathComponent, Path, PathBuf},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Missing,
    Directory,
    File,
    Symlink,
}

pub struct InstallPlan {
    files: [PathBuf; 4],
    review_only: bool,
}
impl InstallPlan {
    pub fn user_local(home: &Path) -> Result<Self, AppError> {
        Self::user_local_xdg(home, &home.join(".local/share"))
    }
    pub fn user_local_xdg(home: &Path, data_home: &Path) -> Result<Self, AppError> {
        Self::validate_root(home)?;
        Self::validate_root(data_home)?;
        Ok(Self::at_paths(&home.join(".local/bin"), data_home))
    }
    fn validate_root(home: &Path) -> Result<(), AppError> {
        let bytes = home.as_os_str().as_bytes();
        if !home.is_absolute()
            || home == Path::new("/")
            || bytes.len() > 4096
            || bytes.iter().any(|byte| *byte < 32 || *byte == 127)
            || home
                .components()
                .any(|part| !matches!(part, PathComponent::RootDir | PathComponent::Normal(_)))
        {
            return Err(AppError::new(Component::Config, ErrorKind::InvalidInput));
        }
        Ok(())
    }
    pub fn system_local() -> Self {
        Self {
            review_only: true,
            ..Self::at(Path::new("/usr/local"))
        }
    }
    /// Stage the system layout under an absolute, operator-controlled directory.
    pub fn staged(root: &Path) -> Result<Self, AppError> {
        Self::user_local(root)?;
        Ok(Self::at(&root.join("usr/local")))
    }
    fn at(prefix: &Path) -> Self {
        Self::at_paths(&prefix.join("bin"), &prefix.join("share"))
    }
    fn at_paths(bin: &Path, data: &Path) -> Self {
        Self {
            files: [
                bin.join("ptyra"),
                data.join("terminfo/p/ptyra"),
                data.join("applications/ptyra.desktop"),
                data.join("ptyra/ptyra.svg"),
            ],
            review_only: false,
        }
    }
    pub fn files(&self) -> &[PathBuf; 4] {
        &self.files
    }
    /// This snapshot authorizes no writes and never treats an existing destination as replaceable.
    pub fn preflight(&self, mut inspect: impl FnMut(&Path) -> EntryKind) -> Result<(), AppError> {
        for file in &self.files {
            if inspect(file) != EntryKind::Missing {
                return Err(AppError::new(Component::Config, ErrorKind::AlreadyExists));
            }
            for parent in file.ancestors().skip(1) {
                if !matches!(inspect(parent), EntryKind::Missing | EntryKind::Directory) {
                    return Err(AppError::new(
                        Component::Config,
                        ErrorKind::PermissionDenied,
                    ));
                }
            }
        }
        Ok(())
    }
    /// Accept only exact paths recorded by a successful install; never remove parent directories.
    pub fn uninstall_paths(&self, installed: &[PathBuf]) -> Result<Vec<PathBuf>, AppError> {
        if installed.len() > self.files.len()
            || installed
                .iter()
                .enumerate()
                .any(|(i, path)| !self.files.contains(path) || installed[..i].contains(path))
        {
            return Err(AppError::new(
                Component::Config,
                ErrorKind::PermissionDenied,
            ));
        }
        Ok(installed.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_points_to_ptyra() {
        let mut lines = include_str!("../assets/ptyra.desktop")
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'));
        assert_eq!(lines.next(), Some("[Desktop Entry]"));
        let mut fields = std::collections::BTreeMap::new();
        for line in lines {
            let (key, value) = line.split_once('=').expect("desktop entry key/value");
            assert!(fields.insert(key, value).is_none(), "duplicate key: {key}");
        }
        // Exact fields exclude shell wrappers, field expansion, actions and D-Bus activation.
        assert_eq!(
            fields,
            std::collections::BTreeMap::from([
                ("Type", "Application"),
                ("Name", crate::APP_NAME),
                ("Exec", env!("CARGO_PKG_NAME")),
                ("TryExec", env!("CARGO_PKG_NAME")),
                ("Icon", "ptyra"),
                ("Terminal", "false"),
                ("Categories", "System;TerminalEmulator;"),
            ])
        );
        let local = InstallPlan::user_local(Path::new("/home/test user")).unwrap();
        let system = InstallPlan::system_local();
        for (plan, desktop) in [
            (
                &local,
                "/home/test user/.local/share/applications/ptyra.desktop",
            ),
            (&system, "/usr/local/share/applications/ptyra.desktop"),
        ] {
            let desktop = PathBuf::from(desktop);
            assert!(plan.files().contains(&desktop));
            assert_eq!(
                plan.uninstall_paths(std::slice::from_ref(&desktop))
                    .unwrap(),
                std::slice::from_ref(&desktop)
            );
            for occupied in [EntryKind::File, EntryKind::Symlink, EntryKind::Directory] {
                assert!(
                    plan.preflight(|path| if path == desktop {
                        occupied
                    } else {
                        EntryKind::Missing
                    })
                    .is_err()
                );
            }
            assert!(
                plan.preflight(|path| if path == desktop.parent().unwrap() {
                    EntryKind::Symlink
                } else {
                    EntryKind::Missing
                })
                .is_err()
            );
            assert!(
                plan.uninstall_paths(&[desktop.with_file_name("other.desktop")])
                    .is_err()
            );
        }
    }

    #[test]
    fn install_paths_do_not_overwrite_system_files() {
        let local = InstallPlan::user_local(Path::new("/home/test user")).unwrap();
        let system = InstallPlan::system_local();
        assert_eq!(
            local.files()[0],
            Path::new("/home/test user/.local/bin/ptyra")
        );
        assert_eq!(system.files()[0], Path::new("/usr/local/bin/ptyra"));
        for plan in [&local, &system] {
            assert!(plan.preflight(|_| EntryKind::Missing).is_ok());
            assert!(
                plan.preflight(|path| if path == plan.files()[0] {
                    EntryKind::File
                } else {
                    EntryKind::Directory
                })
                .is_err()
            );
            assert!(
                plan.preflight(|path| if path == plan.files()[0].parent().unwrap() {
                    EntryKind::Symlink
                } else {
                    EntryKind::Missing
                })
                .is_err()
            );
            assert_eq!(plan.uninstall_paths(plan.files()).unwrap(), plan.files());
            assert!(
                plan.uninstall_paths(&[PathBuf::from("/usr/bin/kitty")])
                    .is_err()
            );
            assert!(
                plan.uninstall_paths(&[plan.files()[0].clone(), plan.files()[0].clone()])
                    .is_err()
            );
        }
        for home in ["/", "relative", "/home/test/../other", "/home/test\nother"] {
            assert!(InstallPlan::user_local(Path::new(home)).is_err());
        }
    }
}
