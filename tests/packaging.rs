#![cfg(all(target_os = "linux", target_env = "gnu"))]

use ptyra::install::InstallPlan;
use std::{fs, path::Path, process::Command};

#[test]
fn install_script_registers_launcher_and_icon() {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let root = nix::unistd::mkdtemp("/tmp/ptyra-launcher-XXXXXX").unwrap();
    let check = || {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
        let home = root.join("home with spaces");
        let data = root.join("custom data");
        let cargo = root.join("cargo");
        fs::write(&cargo, "#!/bin/sh\nexit 99\n").unwrap();
        fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
        let binary = home.join(".local/bin/ptyra");
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        fs::copy(env!("CARGO_BIN_EXE_ptyra"), &binary).unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        let invoke = || {
            Command::new("/bin/sh")
                .arg(repo.join("install.sh"))
                .current_dir(&root)
                .env("HOME", &home)
                .env("XDG_DATA_HOME", &data)
                .env("PATH", format!("{}:/usr/bin:/bin", root.display()))
                .output()
                .unwrap()
        };
        let output = invoke();
        assert!(output.status.success(), "{output:?}");
        let desktop = data.join("applications/ptyra.desktop");
        let icon = data.join("ptyra/ptyra.svg");
        let text = fs::read_to_string(&desktop).unwrap();
        assert!(text.contains(&format!(
            "Exec=\"{}\"",
            home.join(".local/bin/ptyra").display()
        )));
        assert!(
            text.lines()
                .any(|line| line == format!("Icon={}", icon.display()))
        );
        assert_eq!(
            fs::read(&icon).unwrap(),
            fs::read(repo.join("assets/ptyra.svg")).unwrap()
        );
        assert!(
            Command::new("desktop-file-validate")
                .arg(&desktop)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            !invoke().status.success(),
            "existing installation must be preserved"
        );
        assert_eq!(fs::read_to_string(&desktop).unwrap(), text);
        assert_eq!(
            fs::read(data.join("terminfo/p/ptyra")).unwrap(),
            ptyra::terminfo::COMPILED
        );
        fs::remove_file(&desktop).unwrap();
        fs::remove_file(&icon).unwrap();
        let installed_binary = fs::read(&binary).unwrap();
        let output = invoke();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(fs::read(&binary).unwrap(), installed_binary);
        assert_eq!(fs::read_to_string(&desktop).unwrap(), text);
        assert!(icon.is_file());

        let bundle = root.join("bundle");
        fs::create_dir(&bundle).unwrap();
        fs::copy(repo.join("install.sh"), bundle.join("install.sh")).unwrap();
        fs::copy(env!("CARGO_BIN_EXE_ptyra"), bundle.join("ptyra")).unwrap();
        let fresh_home = root.join("fresh home");
        let fresh_data = root.join("fresh data");
        let install_bundle = || {
            Command::new("/bin/sh")
                .arg(bundle.join("install.sh"))
                .env("HOME", &fresh_home)
                .env("XDG_DATA_HOME", &fresh_data)
                .env("PATH", format!("{}:/usr/bin:/bin", root.display()))
                .output()
                .unwrap()
        };
        let fresh_desktop = fresh_data.join("applications/ptyra.desktop");
        let fresh_terminfo = fresh_data.join("terminfo/p/ptyra");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(fresh_terminfo.parent().unwrap())
            .unwrap();
        fs::write(&fresh_terminfo, "keep").unwrap();
        let conflict = install_bundle();
        assert!(!conflict.status.success(), "{conflict:?}");
        assert!(String::from_utf8_lossy(&conflict.stderr).contains("AlreadyExists"));
        assert_eq!(fs::read_to_string(&fresh_terminfo).unwrap(), "keep");
        assert!(!fresh_home.join(".local/bin/ptyra").exists());
        assert!(!fresh_desktop.exists());
        fs::remove_file(&fresh_terminfo).unwrap();
        let output = install_bundle();
        assert!(output.status.success(), "{output:?}");
        assert!(fresh_home.join(".local/bin/ptyra").is_file());
        assert!(fresh_desktop.is_file());
        assert!(fresh_data.join("ptyra/ptyra.svg").is_file());
        assert_eq!(fs::read(fresh_terminfo).unwrap(), ptyra::terminfo::COMPILED);
    };
    let result = std::panic::catch_unwind(check);
    fs::remove_dir_all(root).unwrap();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

#[test]
fn packaged_capabilities_match_runtime_support_on_disk() {
    let root = nix::unistd::mkdtemp("/tmp/ptyra-package-XXXXXX").unwrap();
    let check = || {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
        let output = Command::new("tic")
            .args(["-o"])
            .arg(root.join("terminfo"))
            .arg(repo.join("assets/ptyra.terminfo"))
            .output()
            .expect("packaging verification requires ncurses tic");
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            fs::read(root.join("terminfo/p/ptyra")).unwrap(),
            ptyra::terminfo::COMPILED
        );
        for staged in [false, true] {
            let prefix = root.join(if staged {
                "stage"
            } else {
                r#"home with spaces " $`\"#
            });
            let plan = if staged {
                InstallPlan::staged(&prefix)
            } else {
                InstallPlan::user_local(&prefix)
            }
            .unwrap();
            let receipt = plan
                .install(Path::new(env!("CARGO_BIN_EXE_ptyra")))
                .unwrap();
            assert_eq!(
                fs::read(&plan.files()[1]).unwrap(),
                ptyra::terminfo::COMPILED
            );
            let output = Command::new(&plan.files()[0])
                .current_dir(&root)
                .args(["--config"])
                .arg(root.join("missing.toml"))
                .env_remove("WAYLAND_SOCKET")
                .env("WAYLAND_DISPLAY", root.join("missing-wayland"))
                .output()
                .unwrap();
            assert!(!output.status.success());
            assert!(
                String::from_utf8(output.stderr)
                    .unwrap()
                    .contains("cannot connect to Wayland")
            );
            if std::env::var_os("PTYRA_TEST_PACKAGING_NATIVE").is_some() {
                let output = Command::new("/usr/bin/python3")
                    .arg("-B")
                    .arg(repo.join("tests/native_packaging.py"))
                    .args(plan.files())
                    .current_dir(&root)
                    .output()
                    .unwrap();
                assert!(output.status.success(), "{output:?}");
                print!("{}", String::from_utf8(output.stdout).unwrap());
            }
            plan.uninstall(&receipt).unwrap();
            plan.uninstall(&receipt).unwrap();
            assert!(plan.files().iter().all(|path| !path.exists()));
        }
    };
    let result = std::panic::catch_unwind(check);
    fs::remove_dir_all(root).unwrap();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
