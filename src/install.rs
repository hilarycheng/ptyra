//! Pure install/rollback paths. A filesystem adapter must use exclusive creation and recheck symlink races.
use crate::error::{AppError, Component};
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
    files: [PathBuf; 3],
}
impl InstallPlan {
    pub fn user_local(home: &Path) -> Result<Self, AppError> {
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
        Ok(Self::at(&home.join(".local")))
    }
    pub fn system_local() -> Self {
        Self::at(Path::new("/usr/local"))
    }
    fn at(prefix: &Path) -> Self {
        Self {
            files: [
                prefix.join("bin/ptyra"),
                prefix.join("share/terminfo/p/ptyra"),
                prefix.join("share/applications/ptyra.desktop"),
            ],
        }
    }
    pub fn files(&self) -> &[PathBuf; 3] {
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
