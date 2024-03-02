//! Shared behaviour required for tests.

#[macro_export]
macro_rules! build {
    ($path:literal) => {{
        let mut in_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        in_path.push("tests");
        in_path.push($path);

        let mut out_path = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        out_path.push(format!(
            "test_debugger_{}",
            in_path.file_stem().unwrap().to_str().unwrap()
        ));

        if cfg!(target_family = "windows") {
            out_path.set_extension("exe");
        }

        let rustc = std::process::Command::new("rustc")
            .arg("-Cdebuginfo=2")
            .arg(format!("-o{}", out_path.display()))
            .arg(in_path)
            .output()
            .unwrap();

        if !rustc.stderr.is_empty() {
            eprintln!("{}", String::from_utf8_lossy(&rustc.stderr[..]));
        }

        if !rustc.status.success() {
            panic!("rustc failed with exit code: {}", rustc.status);
        }

        out_path
    }};
}