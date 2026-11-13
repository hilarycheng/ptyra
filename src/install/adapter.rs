use super::InstallPlan;
use std::{
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

mod filesystem;

const MAX_BINARY_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug)]
pub struct InstalledFile {
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl InstalledFile {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug)]
pub struct InstallReceipt {
    files: Vec<InstalledFile>,
}

impl InstallReceipt {
    pub fn files(&self) -> &[InstalledFile] {
        &self.files
    }
}

#[derive(Debug)]
pub struct RollbackFailure {
    pub path: PathBuf,
    pub kind: io::ErrorKind,
}

#[derive(Debug)]
pub struct InstallFailure {
    pub kind: io::ErrorKind,
    pub rollback_failures: Vec<RollbackFailure>,
}

trait InstallFs {
    type Created: Write;

    fn create(&mut self, path: &Path) -> io::Result<Self::Created>;
    fn finish(&mut self, file: &mut Self::Created, mode: u32) -> io::Result<InstalledFile>;
    fn rollback(&mut self, file: Self::Created) -> Result<(), RollbackFailure>;
}

fn execute(
    plan: &InstallPlan,
    binary: &mut dyn Read,
    fs: &mut impl InstallFs,
) -> Result<InstallReceipt, InstallFailure> {
    if plan.review_only {
        return Err(failure(io::ErrorKind::PermissionDenied));
    }
    let mut terminfo = &include_bytes!("../../assets/ptyra.terminfo.bin")[..];
    let mut desktop = &include_bytes!("../../assets/ptyra.desktop")[..];
    let terminfo_len = terminfo.len() as u64;
    let desktop_len = desktop.len() as u64;
    let inputs: [(&mut dyn Read, u64, u32); 3] = [
        (binary, MAX_BINARY_BYTES, 0o755),
        (&mut terminfo, terminfo_len, 0o644),
        (&mut desktop, desktop_len, 0o644),
    ];
    let mut created = Vec::with_capacity(3);
    let result = (|| {
        let mut files = Vec::with_capacity(3);
        for (index, (input, limit, mode)) in inputs.into_iter().enumerate() {
            created.push(fs.create(&plan.files()[index])?);
            let file = &mut created[index];
            copy_bounded(input, file, limit)?;
            files.push(fs.finish(file, mode)?);
        }
        Ok(InstallReceipt { files })
    })();
    result.map_err(|error: io::Error| InstallFailure {
        kind: error.kind(),
        rollback_failures: created
            .into_iter()
            .rev()
            .filter_map(|file| fs.rollback(file).err())
            .collect(),
    })
}

fn copy_bounded(input: &mut dyn Read, output: &mut dyn Write, limit: u64) -> io::Result<()> {
    let count = io::copy(&mut input.take(limit + 1), output)?;
    if count == 0 || count > limit {
        return Err(io::ErrorKind::InvalidData.into());
    }
    output.flush()
}

fn failure(kind: io::ErrorKind) -> InstallFailure {
    InstallFailure {
        kind,
        rollback_failures: Vec::new(),
    }
}

impl InstallPlan {
    /// Install three files without replacement. Existing or created directories remain on failure.
    /// The prefix must be operator-controlled; concurrent writers with the same UID are not isolated.
    /// System plans are review-only. Use `staged` to prepare the system layout without system writes.
    pub fn install(&self, binary: &Path) -> Result<InstallReceipt, InstallFailure> {
        use nix::{
            fcntl::{OFlag, open},
            sys::stat::Mode,
        };
        if self.review_only {
            return Err(failure(io::ErrorKind::PermissionDenied));
        }
        let mut source = open(
            binary,
            OFlag::O_RDONLY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK,
            Mode::empty(),
        )
        .map(File::from)
        .map_err(|error| failure(io::Error::from(error).kind()))?;
        let metadata = source.metadata().map_err(|error| failure(error.kind()))?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_BINARY_BYTES {
            return Err(failure(io::ErrorKind::InvalidInput));
        }
        execute(self, &mut source, &mut filesystem::Filesystem)
    }
}

#[cfg(test)]
mod tests;
