use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "desktoppet-p0-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn rejects(&self, expected: &str) {
        let result = Command::new(env!("CARGO_BIN_EXE_p0-probe"))
            .arg("audit")
            .arg(self.0.join("model.model3.json"))
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(1));
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("panicked"), "{error}");
    }
    fn manifest(&self, moc: &str) {
        fs::write(self.0.join("model.model3.json"), serde_json::to_vec(&serde_json::json!({"Version":3,"FileReferences":{"Moc":moc,"Textures":["texture.png"]}})).unwrap()).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn malformed_json_is_an_error() {
    let f = Fixture::new();
    fs::write(f.0.join("model.model3.json"), b"{").unwrap();
    f.rejects("EOF");
}

#[test]
fn missing_reference_is_an_error() {
    let f = Fixture::new();
    f.manifest("missing.moc3");
    f.rejects("missing asset: missing.moc3");
}

#[test]
fn absolute_reference_is_rejected() {
    let f = Fixture::new();
    f.manifest("/etc/hosts");
    f.rejects("absolute asset path rejected");
}

#[test]
fn parent_traversal_is_rejected() {
    let f = Fixture::new();
    fs::create_dir(f.0.join("nested")).unwrap();
    fs::write(f.0.join("outside.moc3"), b"MOC3").unwrap();
    f.manifest("../outside.moc3");
    fs::rename(
        f.0.join("model.model3.json"),
        f.0.join("nested/model.model3.json"),
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_p0-probe"))
        .arg("audit")
        .arg(f.0.join("nested/model.model3.json"))
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("asset escapes model directory"));
}

#[cfg(unix)]
#[test]
fn symlink_escape_is_rejected() {
    let f = Fixture::new();
    std::os::unix::fs::symlink("/etc/hosts", f.0.join("link.moc3")).unwrap();
    f.manifest("link.moc3");
    f.rejects("asset escapes model directory");
}

#[test]
fn corrupt_moc_is_reported_without_panic() {
    let f = Fixture::new();
    f.manifest("broken.moc3");
    fs::write(f.0.join("broken.moc3"), b"not a model").unwrap();
    image::RgbaImage::new(1, 1)
        .save(f.0.join("texture.png"))
        .unwrap();
    f.rejects("failed to parse moc3");
}
