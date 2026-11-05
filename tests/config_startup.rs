use std::{fs, process::Command};

#[test]
fn startup_reports_bad_config_without_rewriting() {
    let folder = std::env::temp_dir().join(format!("ptyra-config-test-{}", std::process::id()));
    fs::create_dir(&folder).unwrap();
    let path = folder.join("config.toml");
    let missing = Command::new(env!("CARGO_BIN_EXE_ptyra"))
        .arg("--config")
        .arg(&path)
        .output()
        .unwrap();
    assert!(missing.status.success());
    let original = "font_size = 'SECRET'\n";
    fs::write(&path, original).unwrap();
    let bad = Command::new(env!("CARGO_BIN_EXE_ptyra"))
        .arg("--config")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!bad.status.success());
    let diagnostic = String::from_utf8(bad.stderr).unwrap();
    assert!(diagnostic.contains("config.toml"));
    assert!(diagnostic.contains("font_size"));
    assert!(!diagnostic.contains("SECRET"));
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    fs::remove_file(path).unwrap();
    fs::remove_dir(folder).unwrap();
}
