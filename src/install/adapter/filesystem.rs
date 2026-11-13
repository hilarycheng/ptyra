use super::{InstallFs, InstalledFile, RollbackFailure};
use nix::{
    errno::Errno,
    fcntl::{AtFlags, OFlag, RenameFlags, open, openat, renameat2},
    sys::stat::{FileStat, Mode, fstat, fstatat, mkdirat},
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

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct FileStamp {
    size: i64,
    mode: u32,
    uid: u32,
    gid: u32,
    modified: (i64, i64),
}

impl From<&FileStat> for FileStamp {
    fn from(stat: &FileStat) -> Self {
        Self {
            size: stat.st_size,
            mode: stat.st_mode,
            uid: stat.st_uid,
            gid: stat.st_gid,
            modified: (stat.st_mtime, stat.st_mtime_nsec),
        }
    }
}

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
        let installed = InstalledFile::snapshot(&created.path, &fstat(&created.file)?);
        let named = fstatat(
            &created.parent,
            created.path.file_name().unwrap(),
            AtFlags::AT_SYMLINK_NOFOLLOW,
        )?;
        if !installed.unchanged(&named) {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        Ok(installed)
    }

    fn rollback(&mut self, created: CreatedFile) -> Result<(), RollbackFailure> {
        let failed = |kind| RollbackFailure {
            path: created.path.clone(),
            kind,
        };
        let stat = fstat(&created.file).map_err(|error| failed(io::Error::from(error).kind()))?;
        let owned = InstalledFile::snapshot(&created.path, &stat);
        remove_from_parent(&created.parent, &owned)
    }
}

pub(super) fn remove(owned: &InstalledFile) -> Result<(), RollbackFailure> {
    match open_parent(&owned.path, false) {
        Ok(parent) => remove_from_parent(&parent, owned),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(RollbackFailure {
            path: owned.path.clone(),
            kind: error.kind(),
        }),
    }
}

fn remove_from_parent(parent: &File, owned: &InstalledFile) -> Result<(), RollbackFailure> {
    let failed = |kind| RollbackFailure {
        path: owned.path.clone(),
        kind,
    };
    let name = owned.path.file_name().unwrap();
    match fstatat(parent, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Err(Errno::ENOENT) => return Ok(()),
        Ok(named) if owned.unchanged(&named) => (),
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
    let scratch_path = owned.path.with_file_name(&scratch);
    mkdirat(parent, scratch.as_str(), Mode::from_bits_truncate(0o700))
        .map_err(|error| failed(io::Error::from(error).kind()))?;
    let result = capture_and_remove(parent, name, &scratch, owned);
    let cleanup = unlinkat(parent, scratch.as_str(), UnlinkatFlags::RemoveDir);
    match (result, cleanup) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(RollbackFailure {
            path: scratch_path,
            kind: io::Error::from(error).kind(),
        }),
        (Ok(()), Ok(())) => Ok(()),
    }
}

impl InstalledFile {
    fn snapshot(path: &Path, stat: &FileStat) -> Self {
        Self {
            path: path.to_owned(),
            device: stat.st_dev,
            inode: stat.st_ino,
            stamp: FileStamp::from(stat),
            changed: (stat.st_ctime, stat.st_ctime_nsec),
        }
    }

    fn matches(&self, stat: &FileStat) -> bool {
        self.device == stat.st_dev
            && self.inode == stat.st_ino
            && self.stamp == FileStamp::from(stat)
    }

    fn unchanged(&self, stat: &FileStat) -> bool {
        self.matches(stat) && self.changed == (stat.st_ctime, stat.st_ctime_nsec)
    }
}

fn capture_and_remove(
    parent: &File,
    name: &OsStr,
    scratch: &str,
    owned: &InstalledFile,
) -> Result<(), RollbackFailure> {
    let original_failure = |error: Errno| RollbackFailure {
        path: owned.path.clone(),
        kind: io::Error::from(error).kind(),
    };
    let directory =
        openat(parent, scratch, directory_flags(), Mode::empty()).map_err(original_failure)?;
    match renameat2(
        parent,
        name,
        &directory,
        "entry",
        RenameFlags::RENAME_NOREPLACE,
    ) {
        Err(Errno::ENOENT) => return Ok(()),
        result => result.map_err(original_failure)?,
    }
    let captured_failure = |error: Errno| RollbackFailure {
        path: owned.path.with_file_name(scratch).join("entry"),
        kind: io::Error::from(error).kind(),
    };
    let captured =
        fstatat(&directory, "entry", AtFlags::AT_SYMLINK_NOFOLLOW).map_err(captured_failure)?;
    // Rename changes ctime; all other recorded identity and modification fields must still match.
    if !owned.matches(&captured) {
        // Preserve a concurrent replacement; never overwrite a newer destination during restoration.
        renameat2(
            &directory,
            "entry",
            parent,
            name,
            RenameFlags::RENAME_NOREPLACE,
        )
        .map_err(captured_failure)?;
        return Err(RollbackFailure {
            path: owned.path.clone(),
            kind: io::ErrorKind::AlreadyExists,
        });
    }
    unlinkat(&directory, "entry", UnlinkatFlags::NoRemoveDir).map_err(captured_failure)?;
    Ok(())
}
