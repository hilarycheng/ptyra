use std::{fs, process::Command};

#[test]
fn startup_reports_bad_config_without_rewriting() {
    let folder = std::env::temp_dir().join(format!("ptyra-config-test-{}", std::process::id()));
    fs::create_dir(&folder).unwrap();
    let path = folder.join("config.toml");
    let missing = Command::new(env!("CARGO_BIN_EXE_ptyra"))
        .env_remove("WAYLAND_SOCKET")
        .env("WAYLAND_DISPLAY", folder.join("missing-wayland-socket"))
        .arg("--config")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(
        String::from_utf8(missing.stderr)
            .unwrap()
            .contains("cannot connect to Wayland")
    );
    for (original, field) in [
        ("font_size = 'SECRET'\n", "font_size"),
        ("[colors]\nforeground = 'SECRET'\n", "colors.foreground"),
        ("[colors.ansi]\nred = 'SECRET'\n", "colors.ansi"),
    ] {
        fs::write(&path, original).unwrap();
        let bad = Command::new(env!("CARGO_BIN_EXE_ptyra"))
            .arg("--config")
            .arg(&path)
            .output()
            .unwrap();
        assert!(!bad.status.success());
        let diagnostic = String::from_utf8(bad.stderr).unwrap();
        assert!(diagnostic.contains("config.toml"));
        assert!(diagnostic.contains(field));
        assert!(!diagnostic.contains("SECRET"));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }
    let original = "[colors]\nforeground='#123456'\n[colors.ansi]\nbright_blue='#ABCDEF'\n";
    fs::write(&path, original).unwrap();
    let valid = Command::new(env!("CARGO_BIN_EXE_ptyra"))
        .env_remove("WAYLAND_SOCKET")
        .env("WAYLAND_DISPLAY", folder.join("missing-wayland-socket"))
        .arg("--config")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!valid.status.success());
    assert!(
        String::from_utf8(valid.stderr)
            .unwrap()
            .contains("cannot connect to Wayland")
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    fs::remove_file(path).unwrap();
    fs::remove_dir(folder).unwrap();
}
