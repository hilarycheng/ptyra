use super::{InstallFs, InstalledFile, RollbackFailure};
use nix::{
    errno::Errno,
    fcntl::{AtFlags, OFlag, RenameFlags, open, openat, renameat2},
    sys::stat::{FileStat, Mode, fstatat, mkdirat},
    unistd::{UnlinkatFlags, geteuid, unlinkat},
};
use std::{
    ffi::OsStr,
    fs::{File, Permissions},
    io::{self, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub(super) struct Filesystem;

pub(super) struct CreatedFile {
    path: PathBuf,
    parent: File,
    file: File,
}

impl Write for CreatedFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.file.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn directory_flags() -> OFlag {
    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW
}

fn open_parent(path: &Path, create: bool) -> io::Result<File> {
    let mut directory = File::from(open(Path::new("/"), directory_flags(), Mode::empty())?);
    for part in path
        .parent()
        .ok_or(io::ErrorKind::InvalidInput)?
        .components()
        .skip(1)
    {
        let Component::Normal(name) = part else {
            return Err(io::ErrorKind::InvalidInput.into());
        };
        let next = match openat(&directory, name, directory_flags(), Mode::empty()) {
            Err(Errno::ENOENT) if create => {
                match mkdirat(&directory, name, Mode::from_bits_truncate(0o755)) {
                    Ok(()) | Err(Errno::EEXIST) => (),
                    Err(error) => return Err(error.into()),
                }
                openat(&directory, name, directory_flags(), Mode::empty())?
            }
            result => result?,
        };
        directory = File::from(next);
        let metadata = directory.metadata()?;
        if ![0, geteuid().as_raw()].contains(&metadata.uid())
            || (metadata.mode() & 0o022 != 0 && metadata.mode() & 0o1000 == 0)
        {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
    }
    // Untrusted users must not be able to replace entries during installation or rollback.
    if directory.metadata()?.mode() & 0o022 != 0 {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(directory)
}

impl InstallFs for Filesystem {
    type Created = CreatedFile;

    fn create(&mut self, path: &Path) -> io::Result<CreatedFile> {
        let parent = open_parent(path, true)?;
        let file = File::from(openat(
            &parent,
            path.file_name().ok_or(io::ErrorKind::InvalidInput)?,
            OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )?);
        Ok(CreatedFile {
            path: path.to_owned(),
            parent,
            file,
        })
    }

    fn finish(&mut self, created: &mut CreatedFile, mode: u32) -> io::Result<InstalledFile> {
        let parent = open_parent(&created.path, false)?.metadata()?;
        let pinned = created.parent.metadata()?;
        if (parent.dev(), parent.ino()) != (pinned.dev(), pinned.ino()) {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        created.file.set_permissions(Permissions::from_mode(mode))?;
        created.file.sync_all()?;
        let metadata = created.file.metadata()?;
        let installed = InstalledFile {
            path: created.path.clone(),
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        let named = fstatat(
            &created.parent,
            created.path.file_name().unwrap(),
            AtFlags::AT_SYMLINK_NOFOLLOW,
        )?;
        if !installed.matches(&named) {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        Ok(installed)
    }

    fn rollback(&mut self, created: CreatedFile) -> Result<(), RollbackFailure> {
        let failed = |kind| RollbackFailure {
            path: created.path.clone(),
            kind,
        };
        let name = created.path.file_name().unwrap();
        let metadata = created
            .file
            .metadata()
            .map_err(|error| failed(error.kind()))?;
        let owned = InstalledFile {
            path: created.path.clone(),
            device: metadata.dev(),
            inode: metadata.ino(),
        };
        match fstatat(&created.parent, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
            Err(Errno::ENOENT) => return Ok(()),
            Ok(named) if owned.matches(&named) => (),
            Ok(_) => return Err(failed(io::ErrorKind::AlreadyExists)),
            Err(error) => return Err(failed(io::Error::from(error).kind())),
        }

        // A stat/unlink pair could delete a replacement. Capture the entry privately before checking it.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let scratch = format!(
            ".ptyra-rollback-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let scratch_path = created.path.with_file_name(&scratch);
        mkdirat(
            &created.parent,
            scratch.as_str(),
            Mode::from_bits_truncate(0o700),
        )
        .map_err(|error| failed(io::Error::from(error).kind()))?;
        let result = capture_and_remove(&created, name, &scratch, &owned);
        let cleanup = unlinkat(&created.parent, scratch.as_str(), UnlinkatFlags::RemoveDir);
        match (result, cleanup) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(RollbackFailure {
                path: scratch_path,
                kind: io::Error::from(error).kind(),
            }),
            (Ok(()), Ok(())) => Ok(()),
        }
    }
}

impl InstalledFile {
    fn matches(&self, stat: &FileStat) -> bool {
        self.device == stat.st_dev && self.inode == stat.st_ino
    }
}

fn capture_and_remove(
    created: &CreatedFile,
    name: &OsStr,
    scratch: &str,
    owned: &InstalledFile,
) -> Result<(), RollbackFailure> {
    let original_failure = |error: Errno| RollbackFailure {
        path: created.path.clone(),
        kind: io::Error::from(error).kind(),
    };
    let directory = openat(&created.parent, scratch, directory_flags(), Mode::empty())
        .map_err(original_failure)?;
    match renameat2(
        &created.parent,
        name,
        &directory,
        "entry",
        RenameFlags::RENAME_NOREPLACE,
    ) {
        Err(Errno::ENOENT) => return Ok(()),
        result => result.map_err(original_failure)?,
    }
    let captured_failure = |error: Errno| RollbackFailure {
        path: created.path.with_file_name(scratch).join("entry"),
        kind: io::Error::from(error).kind(),
    };
    let captured =
        fstatat(&directory, "entry", AtFlags::AT_SYMLINK_NOFOLLOW).map_err(captured_failure)?;
    if !owned.matches(&captured) {
        // Preserve a concurrent replacement; never overwrite a newer destination during restoration.
        renameat2(
            &directory,
            "entry",
            &created.parent,
            name,
            RenameFlags::RENAME_NOREPLACE,
        )
        .map_err(captured_failure)?;
        return Ok(());
    }
    unlinkat(&directory, "entry", UnlinkatFlags::NoRemoveDir).map_err(captured_failure)?;
    Ok(())
}
