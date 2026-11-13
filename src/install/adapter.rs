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
    stamp: filesystem::FileStamp,
    changed: (i64, i64),
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
    let desktop_entry = plan
        .desktop_entry()
        .map_err(|error| failure(error.kind()))?;
    let mut terminfo = crate::terminfo::COMPILED;
    let mut desktop = desktop_entry.as_bytes();
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
    fn desktop_entry(&self) -> io::Result<String> {
        // shortcut: reject '%' until launchers resolve escaped executable paths reliably.
        let binary = self.files()[0]
            .to_str()
            .filter(|path| !path.contains(['=', '%']))
            .ok_or(io::ErrorKind::InvalidInput)?;
        let mut command = String::from("\"");
        for character in binary.chars() {
            if matches!(character, '\\' | '"' | '$' | '`') {
                command.push('\\');
            }
            command.push(character);
        }
        command.push('"');
        // Desktop string escaping precedes Exec argument unquoting.
        Ok(include_str!("../../assets/ptyra.desktop")
            .replace(
                "\nExec=ptyra\n",
                &format!("\nExec={}\n", command.replace('\\', "\\\\")),
            )
            .replace(
                "\nTryExec=ptyra\n",
                &format!("\nTryExec={}\n", binary.replace('\\', "\\\\")),
            ))
    }

    /// Remove unchanged files from this plan's successful, in-memory receipt.
    /// Missing files are harmless. Preserve directories and report every refused removal.
    pub fn uninstall(&self, receipt: &InstallReceipt) -> Result<(), Vec<RollbackFailure>> {
        uninstall(self, receipt, filesystem::remove)
    }

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

fn uninstall(
    plan: &InstallPlan,
    receipt: &InstallReceipt,
    mut remove: impl FnMut(&InstalledFile) -> Result<(), RollbackFailure>,
) -> Result<(), Vec<RollbackFailure>> {
    if plan.review_only
        || receipt.files.len() != plan.files().len()
        || receipt
            .files
            .iter()
            .zip(plan.files())
            .any(|(file, path)| file.path != *path)
    {
        return Err(vec![RollbackFailure {
            path: plan.files()[0].clone(),
            kind: io::ErrorKind::PermissionDenied,
        }]);
    }
    let errors: Vec<_> = receipt
        .files
        .iter()
        .rev()
        .filter_map(|file| remove(file).err())
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests;
