#![cfg(all(target_os = "linux", target_env = "gnu"))]

use ptyra::install::InstallPlan;
use std::{fs, path::Path, process::Command};

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
