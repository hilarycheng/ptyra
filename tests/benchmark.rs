#[test]
fn benchmark_samples_report_single_core_cpu_and_pss() {
    let result = std::process::Command::new("python3")
        .arg("-B")
        .arg("tests/benchmark_samples.py")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("benchmark fixture checks require Python 3");
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
