use super::*;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

type Files = Rc<RefCell<BTreeMap<PathBuf, Vec<u8>>>>;

#[test]
fn packaged_capabilities_match_runtime_support() {
    for home in ["/home/test user", r#"/home/a\b\"$`"#] {
        let plan = InstallPlan::user_local(Path::new(home)).unwrap();
        let mut fs = FakeFs::new();
        execute(&plan, &mut &b"binary"[..], &mut fs).unwrap();
        let files = fs.files.borrow();
        let desktop = std::str::from_utf8(&files[&plan.files()[2]]).unwrap();
        assert!(!desktop.lines().any(|line| line == "Exec=ptyra"));
        assert_eq!(files[&plan.files()[1]], crate::terminfo::COMPILED);
        assert_eq!(
            files[&plan.files()[2]],
            plan.desktop_entry().unwrap().as_bytes()
        );
    }
    let plain = InstallPlan::user_local(Path::new("/home/test user")).unwrap();
    assert!(plain.desktop_entry().unwrap().contains(
        "Exec=\"/home/test user/.local/bin/ptyra\"\nTryExec=/home/test user/.local/bin/ptyra\n"
    ));
    let special = InstallPlan::user_local(Path::new(r#"/home/a\b\"$`"#)).unwrap();
    assert!(
        special
            .desktop_entry()
            .unwrap()
            .contains(r#"Exec="/home/a\\\\b\\\\\\"\\$\\`/.local/bin/ptyra""#)
    );
    for home in ["/home/a=b", "/home/a%fb"] {
        let invalid = InstallPlan::user_local(Path::new(home)).unwrap();
        let mut fs = FakeFs::new();
        assert_eq!(
            execute(&invalid, &mut &b"binary"[..], &mut fs)
                .unwrap_err()
                .kind,
            io::ErrorKind::InvalidInput
        );
        assert!(fs.files.borrow().is_empty());
    }
}

struct FakeFs {
    files: Files,
    fail_create: Option<usize>,
    fail_write: Option<usize>,
    fail_finish: Option<usize>,
    fail_rollback: Option<usize>,
    creates: usize,
    removed: Vec<PathBuf>,
    modes: Vec<u32>,
}

impl FakeFs {
    fn new() -> Self {
        Self {
            files: Rc::default(),
            fail_create: None,
            fail_write: None,
            fail_finish: None,
            fail_rollback: None,
            creates: 0,
            removed: Vec::new(),
            modes: Vec::new(),
        }
    }
}

struct FakeFile {
    files: Files,
    path: PathBuf,
    index: usize,
    fail_write: bool,
    interrupted: bool,
}

impl Write for FakeFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !self.interrupted {
            self.interrupted = true;
            return Err(io::ErrorKind::Interrupted.into());
        }
        let mut files = self.files.borrow_mut();
        let contents = files.get_mut(&self.path).unwrap();
        if self.fail_write && !contents.is_empty() {
            return Err(io::ErrorKind::StorageFull.into());
        }
        let length = bytes.len().min(3);
        contents.extend_from_slice(&bytes[..length]);
        Ok(length)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl InstallFs for FakeFs {
    type Created = FakeFile;

    fn create(&mut self, path: &Path) -> io::Result<FakeFile> {
        let index = self.creates;
        self.creates += 1;
        if self.files.borrow().contains_key(path) {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        if self.fail_create == Some(index) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        self.files.borrow_mut().insert(path.to_owned(), Vec::new());
        Ok(FakeFile {
            files: Rc::clone(&self.files),
            path: path.to_owned(),
            index,
            fail_write: self.fail_write == Some(index),
            interrupted: false,
        })
    }

    fn finish(&mut self, file: &mut FakeFile, mode: u32) -> io::Result<InstalledFile> {
        if self.fail_finish == Some(file.index) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        self.modes.push(mode);
        Ok(InstalledFile {
            path: file.path.clone(),
            device: 1,
            inode: file.index as u64 + 1,
            stamp: filesystem::FileStamp::default(),
            changed: (0, 0),
        })
    }

    fn rollback(&mut self, file: FakeFile) -> Result<(), RollbackFailure> {
        if self.fail_rollback == Some(file.index) {
            return Err(RollbackFailure {
                path: file.path,
                kind: io::ErrorKind::PermissionDenied,
            });
        }
        assert!(self.files.borrow_mut().remove(&file.path).is_some());
        self.removed.push(file.path);
        Ok(())
    }
}

#[test]
fn install_adapter_rolls_back_only_created_files() {
    let plan = InstallPlan::user_local(Path::new("/home/test user")).unwrap();
    let unrelated = PathBuf::from("/home/test user/.local/bin/other");
    let binary = b"binary fixture";
    for index in 0..3 {
        for failure in ["create", "write", "finish", "occupied"] {
            let mut fs = FakeFs::new();
            fs.files
                .borrow_mut()
                .insert(unrelated.clone(), b"keep".to_vec());
            match failure {
                "create" => fs.fail_create = Some(index),
                "write" => fs.fail_write = Some(index),
                "finish" => fs.fail_finish = Some(index),
                "occupied" => {
                    fs.files
                        .borrow_mut()
                        .insert(plan.files()[index].clone(), b"original".to_vec());
                }
                _ => unreachable!(),
            }
            let original = fs.files.borrow().clone();
            let error = execute(&plan, &mut &binary[..], &mut fs).unwrap_err();
            assert_ne!(error.kind, io::ErrorKind::Unsupported);
            assert!(error.rollback_failures.is_empty());
            assert_eq!(*fs.files.borrow(), original, "{failure} at {index}");
            let created = index + usize::from(matches!(failure, "write" | "finish"));
            assert_eq!(
                fs.removed,
                plan.files()[..created]
                    .iter()
                    .rev()
                    .cloned()
                    .collect::<Vec<_>>()
            );
        }
    }

    let mut fs = FakeFs::new();
    fs.fail_finish = Some(2);
    fs.fail_rollback = Some(1);
    let error = execute(&plan, &mut &binary[..], &mut fs).unwrap_err();
    assert_eq!(error.rollback_failures.len(), 1);
    assert_eq!(error.rollback_failures[0].path, plan.files()[1]);
    assert_eq!(
        fs.removed,
        [plan.files()[2].clone(), plan.files()[0].clone()]
    );
    assert_eq!(fs.files.borrow().len(), 1);

    let mut fs = FakeFs::new();
    let receipt = execute(&plan, &mut &binary[..], &mut fs).unwrap();
    assert_eq!(
        receipt
            .files()
            .iter()
            .map(InstalledFile::path)
            .collect::<Vec<_>>(),
        plan.files()
    );
    assert_eq!(fs.modes, [0o755, 0o644, 0o644]);
    assert!(fs.removed.is_empty());
    let files = fs.files.borrow();
    assert_eq!(files[&plan.files()[0]], binary);
    assert_eq!(
        files[&plan.files()[1]],
        include_bytes!("../../../assets/ptyra.terminfo.bin")
    );
    assert_eq!(
        files[&plan.files()[2]],
        plan.desktop_entry().unwrap().as_bytes()
    );
}

#[test]
fn install_adapter_rolls_back_only_created_files_on_disk() {
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };
    let root = TestDirectory::new();
    let binary = root.0.join("source");
    fs::write(&binary, b"binary fixture").unwrap();
    for staged in [false, true] {
        let prefix = root.0.join(if staged { "stage" } else { "home" });
        let plan = if staged {
            InstallPlan::staged(&prefix)
        } else {
            InstallPlan::user_local(&prefix)
        }
        .unwrap();
        let receipt = plan.install(&binary).unwrap();
        assert_eq!(receipt.files().len(), 3);
        for (index, file) in receipt.files().iter().enumerate() {
            assert_eq!(file.path(), plan.files()[index]);
            let expected = if index == 0 { 0o755 } else { 0o644 };
            assert_eq!(
                fs::metadata(file.path()).unwrap().permissions().mode() & 0o777,
                expected
            );
        }
        assert_eq!(fs::read(&plan.files()[0]).unwrap(), b"binary fixture");
        assert_eq!(
            fs::read(&plan.files()[1]).unwrap(),
            include_bytes!("../../../assets/ptyra.terminfo.bin")
        );
        assert_eq!(
            fs::read(&plan.files()[2]).unwrap(),
            plan.desktop_entry().unwrap().as_bytes()
        );
        assert_eq!(
            plan.install(&binary).unwrap_err().kind,
            io::ErrorKind::AlreadyExists
        );
        // A conflict after binary creation removes only the new binary.
        fs::remove_file(&plan.files()[0]).unwrap();
        let error = plan.install(&binary).unwrap_err();
        assert_eq!(error.kind, io::ErrorKind::AlreadyExists);
        assert!(error.rollback_failures.is_empty());
        assert!(!plan.files()[0].exists());
        assert!(plan.files()[1].exists());
        assert!(plan.files()[2].exists());
    }
    let plan = InstallPlan::user_local(&root.0.join("symlink-home")).unwrap();
    fs::create_dir_all(plan.files()[0].parent().unwrap()).unwrap();
    symlink(&binary, &plan.files()[0]).unwrap();
    assert!(plan.install(&binary).is_err());
    assert_eq!(fs::read_link(&plan.files()[0]).unwrap(), binary);
    fs::remove_file(&plan.files()[0]).unwrap();
    let parent = plan.files()[0].parent().unwrap();
    fs::remove_dir(parent).unwrap();
    symlink(&root.0, parent).unwrap();
    assert!(plan.install(&binary).is_err());
    assert!(!root.0.join("ptyra").exists());
    assert_eq!(fs::read(&binary).unwrap(), b"binary fixture");

    let plan = InstallPlan::user_local(&root.0.join("invalid-source")).unwrap();
    let source_link = root.0.join("source-link");
    symlink(&binary, &source_link).unwrap();
    assert!(plan.install(&source_link).is_err());
    assert!(plan.install(&root.0).is_err());
    fs::write(&binary, b"").unwrap();
    assert!(plan.install(&binary).is_err());
    File::options()
        .write(true)
        .open(&binary)
        .unwrap()
        .set_len(MAX_BINARY_BYTES + 1)
        .unwrap();
    assert!(plan.install(&binary).is_err());
    assert!(!plan.files()[0].exists());
    assert_eq!(
        InstallPlan::system_local()
            .install(&binary)
            .unwrap_err()
            .kind,
        io::ErrorKind::PermissionDenied
    );
}

#[test]
fn install_adapter_keeps_replacements_and_pins_parent_during_rollback() {
    use std::{fs, os::unix::fs::symlink};
    let root = TestDirectory::new();
    let path = root.0.join("parent/ptyra");
    let mut fs = filesystem::Filesystem;
    let created = fs.create(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::write(&path, b"replacement").unwrap();
    assert_eq!(
        fs.rollback(created).unwrap_err().kind,
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
    std::fs::remove_file(&path).unwrap();
    let mut created = fs.create(&path).unwrap();
    created.write_all(b"owned").unwrap();
    let moved = root.0.join("moved");
    std::fs::rename(path.parent().unwrap(), &moved).unwrap();
    let other = root.0.join("other");
    std::fs::create_dir(&other).unwrap();
    std::fs::write(other.join("ptyra"), b"keep").unwrap();
    symlink(&other, path.parent().unwrap()).unwrap();
    assert!(fs.finish(&mut created, 0o755).is_err());
    fs.rollback(created).unwrap();
    assert!(!moved.join("ptyra").exists());
    assert_eq!(std::fs::read(&path).unwrap(), b"keep");
    fs::remove_file(path.parent().unwrap()).unwrap();
}

struct TestDirectory(PathBuf);

#[test]
fn uninstall_preserves_unowned_or_changed_files() {
    let plan = InstallPlan::user_local(Path::new("/home/test user")).unwrap();
    let mut fs = FakeFs::new();
    let mut receipt = execute(&plan, &mut &b"binary"[..], &mut fs).unwrap();
    let mut removed = Vec::new();
    let errors = uninstall(&plan, &receipt, |file| {
        removed.push(file.path.clone());
        Err(RollbackFailure {
            path: file.path.clone(),
            kind: io::ErrorKind::AlreadyExists,
        })
    })
    .unwrap_err();
    assert_eq!(errors.len(), 3);
    assert_eq!(
        removed,
        plan.files().iter().rev().cloned().collect::<Vec<_>>()
    );
    receipt.files[0].path = PathBuf::from("/usr/bin/other");
    assert!(
        uninstall(&plan, &receipt, |_| panic!(
            "invalid receipt reached filesystem"
        ))
        .is_err()
    );
    receipt.files[0].path = plan.files()[1].clone();
    assert!(
        uninstall(&plan, &receipt, |_| panic!(
            "duplicate path reached filesystem"
        ))
        .is_err()
    );
    receipt.files.pop();
    assert!(
        uninstall(&plan, &receipt, |_| panic!(
            "partial receipt reached filesystem"
        ))
        .is_err()
    );
}

#[test]
fn uninstall_preserves_unowned_or_changed_files_on_disk() {
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };
    let root = TestDirectory::new();
    let binary = root.0.join("source");
    fs::write(&binary, b"binary fixture").unwrap();
    for change in [
        "none", "missing", "replace", "edit", "mode", "symlink", "parent",
    ] {
        let plan = InstallPlan::user_local(&root.0.join(change)).unwrap();
        let receipt = plan.install(&binary).unwrap();
        let path = &plan.files()[0];
        let unrelated = path.with_file_name("other");
        fs::write(&unrelated, b"keep").unwrap();
        match change {
            "missing" => fs::remove_file(path).unwrap(),
            "replace" => {
                fs::rename(path, path.with_file_name("original")).unwrap();
                fs::write(path, b"binary fixture").unwrap();
            }
            "edit" => fs::write(path, b"edited fixture").unwrap(),
            "mode" => fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap(),
            "symlink" => {
                fs::remove_file(path).unwrap();
                symlink(&binary, path).unwrap();
            }
            "parent" => {
                fs::rename(
                    path.parent().unwrap(),
                    path.parent().unwrap().with_file_name("moved"),
                )
                .unwrap();
                symlink(
                    path.parent().unwrap().with_file_name("moved"),
                    path.parent().unwrap(),
                )
                .unwrap();
            }
            _ => (),
        }
        for _ in 0..2 {
            let result = plan.uninstall(&receipt);
            if matches!(change, "none" | "missing") {
                assert!(result.is_ok(), "{change}: {result:?}");
                assert!(!path.exists());
            } else {
                let errors = result.unwrap_err();
                assert_eq!(errors.len(), 1, "{change}");
                assert_eq!(errors[0].path, *path);
                assert!(path.exists(), "{change}");
            }
            assert!(path.parent().unwrap().exists());
            assert_eq!(fs::read(&unrelated).unwrap(), b"keep");
            assert!(!plan.files()[1].exists());
            assert!(!plan.files()[2].exists());
        }
        assert_eq!(fs::read(&binary).unwrap(), b"binary fixture");
    }
    let plan = InstallPlan::staged(&root.0.join("staged")).unwrap();
    let receipt = plan.install(&binary).unwrap();
    let other = InstallPlan::user_local(&root.0.join("other")).unwrap();
    assert!(other.uninstall(&receipt).is_err());
    assert!(InstallPlan::system_local().uninstall(&receipt).is_err());
    assert!(plan.files().iter().all(|path| path.is_file()));
    plan.uninstall(&receipt).unwrap();
    plan.uninstall(&receipt).unwrap();
}

impl TestDirectory {
    fn new() -> Self {
        use std::os::unix::fs::DirBuilderExt;
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ptyra-install-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
