//! Versioned local directory packs. No archives, scripts, or implicit migration.
use anyhow::{Context, Result, ensure};
use pet_protocol::{AvatarCapabilities, Feedback, HitRegion};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

pub const MAX_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_JSON: u64 = 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub id: String,
    pub display_name: String,
    pub renderer: String,
    pub entry: String,
    pub interaction: Interaction,
    pub actions: Actions,
    #[serde(default)]
    pub parameter_map: BTreeMap<String, String>,
    pub license: License,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct License {
    pub status: String,
    pub redistributable: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interaction {
    /// Top-left normalized coordinates in the fixed 5:6 pet viewport.
    pub head: Vec<[f64; 2]>,
    pub body: Vec<[f64; 2]>,
    pub anchor: [f64; 2],
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actions {
    pub head_pat: Option<Action>,
    pub body_tap: Option<Action>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub expression: String,
    pub duration_ms: u32,
}
#[derive(Clone, Debug)]
pub struct Pack {
    pub root: PathBuf,
    pub manifest: Manifest,
    pub entry: PathBuf,
}
impl Manifest {
    pub fn capabilities(&self) -> AvatarCapabilities {
        AvatarCapabilities {
            head_pat: self.actions.head_pat.is_some(),
            body_tap: self.actions.body_tap.is_some(),
        }
    }
    pub fn action(&self, feedback: Feedback) -> Option<&Action> {
        match feedback {
            Feedback::HeadPat => self.actions.head_pat.as_ref(),
            Feedback::BodyTap => self.actions.body_tap.as_ref(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema_version == 1,
            "unsupported pack schema_version; expected 1"
        );
        ensure!(self.renderer == "live2d_mocari", "unsupported renderer");
        ensure!(
            !self.id.is_empty()
                && self.id.len() <= 64
                && self
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "invalid pack id"
        );
        ensure!(
            !self.display_name.trim().is_empty() && self.display_name.len() <= 128,
            "invalid display_name"
        );
        ensure!(
            ["unverified", "licensed", "original"].contains(&self.license.status.as_str()),
            "invalid license status"
        );
        ensure!(
            self.license.status != "unverified" || !self.license.redistributable,
            "unverified assets cannot be marked redistributable"
        );
        for polygon in [&self.interaction.head, &self.interaction.body] {
            ensure!(
                (3..=32).contains(&polygon.len()),
                "polygon requires 3–32 points"
            );
            ensure!(
                polygon
                    .iter()
                    .flatten()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                "polygon coordinates must be 0–1"
            );
            // v1 requires convex polygons: simple, deterministic hit testing.
            let mut sign = 0.0_f64;
            for i in 0..polygon.len() {
                let a = polygon[i];
                let b = polygon[(i + 1) % polygon.len()];
                let c = polygon[(i + 2) % polygon.len()];
                let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
                ensure!(
                    cross.abs() > 1e-8 && (sign == 0.0 || cross.signum() == sign),
                    "polygon must be strictly convex"
                );
                sign = cross.signum();
                for point in polygon {
                    let side =
                        (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0]);
                    ensure!(
                        side.abs() < 1e-8 || side.signum() == sign,
                        "self-intersecting polygon rejected"
                    );
                }
            }
        }
        ensure!(
            self.interaction
                .anchor
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "invalid anchor"
        );
        ensure!(self.parameter_map.len() <= 32, "too many mapped parameters");
        for action in [&self.actions.head_pat, &self.actions.body_tap]
            .into_iter()
            .flatten()
        {
            ensure!(
                (100..=5000).contains(&action.duration_ms),
                "action duration must be 100–5000 ms"
            );
        }
        Ok(())
    }
}
impl Interaction {
    /// Retain hover within a small logical-pixel margin; actual clicks still use hit().
    pub fn near(&self, point: [f64; 2], viewport: [f64; 2], margin: f64) -> bool {
        if self.hit(point).is_some() {
            return true;
        }
        if margin <= 0.0
            || !margin.is_finite()
            || !viewport.iter().all(|v| v.is_finite() && *v > 0.0)
        {
            return false;
        }
        for polygon in [&self.head, &self.body] {
            for i in 0..polygon.len() {
                let a = polygon[i];
                let b = polygon[(i + 1) % polygon.len()];
                let edge = [(b[0] - a[0]) * viewport[0], (b[1] - a[1]) * viewport[1]];
                let p = [
                    (point[0] - a[0]) * viewport[0],
                    (point[1] - a[1]) * viewport[1],
                ];
                let length = edge[0] * edge[0] + edge[1] * edge[1];
                if length <= 0.0 {
                    continue;
                }
                let t = ((p[0] * edge[0] + p[1] * edge[1]) / length).clamp(0.0, 1.0);
                if (p[0] - t * edge[0]).powi(2) + (p[1] - t * edge[1]).powi(2) <= margin * margin {
                    return true;
                }
            }
        }
        false
    }
    pub fn hit(&self, point: [f64; 2]) -> Option<HitRegion> {
        if inside(&self.head, point) {
            Some(HitRegion::Head)
        } else if inside(&self.body, point) {
            Some(HitRegion::Body)
        } else {
            None
        }
    }
}
fn inside(polygon: &[[f64; 2]], point: [f64; 2]) -> bool {
    if !point.iter().all(|v| v.is_finite()) {
        return false;
    }
    let mut sign = 0.0_f64;
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[(i + 1) % polygon.len()];
        let cross = (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0]);
        if cross.abs() < 1e-10 {
            continue;
        }
        if sign != 0.0 && cross.signum() != sign {
            return false;
        }
        sign = cross.signum();
    }
    true
}
/// Reject traversal and every symlink component, including links still inside.
pub fn checked_path(root: &Path, name: &str) -> Result<PathBuf> {
    let relative = Path::new(name);
    ensure!(
        !name.is_empty()
            && !name.contains('\\')
            && relative
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
        "invalid relative asset path: {name}"
    );
    let mut path = root.to_owned();
    for component in relative.components() {
        path.push(component);
        ensure!(
            !fs::symlink_metadata(&path)
                .with_context(|| format!("missing asset: {name}"))?
                .file_type()
                .is_symlink(),
            "symlink asset rejected: {name}"
        );
    }
    ensure!(path.is_file(), "asset is not a file: {name}");
    Ok(path)
}
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_JSON + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_JSON, "JSON size limit exceeded");
    serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON: {}", path.display()))
}
fn inventory(root: &Path) -> Result<Vec<PathBuf>> {
    fn walk(
        root: &Path,
        current: &Path,
        paths: &mut Vec<PathBuf>,
        total: &mut u64,
        depth: usize,
        entries: &mut usize,
    ) -> Result<()> {
        ensure!(depth <= 12, "pack directory nesting exceeds limit");
        for item in fs::read_dir(current)? {
            let item = item?;
            *entries += 1;
            ensure!(*entries <= 2048, "pack entry count exceeded");
            let path = item.path();
            let meta = fs::symlink_metadata(&path)?;
            ensure!(
                !meta.file_type().is_symlink(),
                "symlinks are not allowed in packs"
            );
            if meta.is_dir() {
                walk(root, &path, paths, total, depth + 1, entries)?;
            } else {
                ensure!(meta.is_file(), "non-regular pack file");
                ensure!(
                    matches!(
                        path.extension().and_then(|s| s.to_str()),
                        Some("json" | "png" | "moc3")
                    ),
                    "unsupported pack file: {}",
                    path.display()
                );
                let limit = if path.extension().is_some_and(|s| s == "json") {
                    MAX_JSON
                } else {
                    32 * 1024 * 1024
                };
                ensure!(meta.len() <= limit, "asset file size exceeds limit");
                *total = total
                    .checked_add(meta.len())
                    .context("pack size overflow")?;
                ensure!(
                    *total <= MAX_BYTES && paths.len() < 1024,
                    "pack budget exceeded"
                );
                paths.push(path.strip_prefix(root)?.to_owned());
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(root, root, &mut files, &mut 0, 0, &mut 0)?;
    files.sort();
    Ok(files)
}
pub fn open(manifest: &Path) -> Result<Pack> {
    ensure!(
        !fs::symlink_metadata(manifest)?.file_type().is_symlink(),
        "manifest symlink rejected"
    );
    let manifest_path = manifest.canonicalize()?;
    let root = manifest_path
        .parent()
        .context("missing pack root")?
        .to_owned();
    ensure!(
        manifest_path
            .file_name()
            .is_some_and(|n| n == "manifest.json"),
        "select manifest.json"
    );
    let files = inventory(&root)?;
    let manifest: Manifest = read_json(&manifest_path)?;
    manifest.validate()?;
    let entry = checked_path(&root, &manifest.entry)?;
    let model: serde_json::Value = read_json(&entry)?;
    ensure!(model["Version"] == 3, "expected model3 Version 3");
    let refs = model["FileReferences"]
        .as_object()
        .context("missing FileReferences")?;
    let base = entry.parent().context("missing model root")?;
    let reference = |name: &str| -> Result<PathBuf> { checked_path(base, name) };
    let moc = refs
        .get("Moc")
        .and_then(|v| v.as_str())
        .context("missing Moc")?;
    reference(moc)?;
    let textures = refs
        .get("Textures")
        .and_then(|v| v.as_array())
        .context("missing Textures")?;
    ensure!(
        !textures.is_empty() && textures.len() <= 32,
        "expected 1–32 textures"
    );
    let mut decoded = 0u64;
    // Inspect every PNG before any runtime decoding (including unreferenced files).
    for file in &files {
        if file.extension().is_some_and(|s| s == "png") {
            let (w, h) = image::ImageReader::open(root.join(file))?
                .with_guessed_format()?
                .into_dimensions()?;
            ensure!(
                w > 0 && h > 0 && w <= 8192 && h <= 8192,
                "texture dimensions exceed 8192"
            );
            decoded += u64::from(w) * u64::from(h) * 4;
            ensure!(
                decoded <= 128 * 1024 * 1024,
                "decoded texture budget exceeded"
            );
        }
    }
    let mut unique = std::collections::HashSet::new();
    for texture in textures {
        let path = reference(texture.as_str().context("invalid texture path")?)?;
        ensure!(unique.insert(path.clone()), "duplicate texture reference");
        ensure!(
            path.extension().is_some_and(|s| s == "png"),
            "only PNG textures supported"
        );
    }
    for key in ["Physics", "Pose", "DisplayInfo", "UserData"] {
        if let Some(value) = refs.get(key) {
            let _: serde_json::Value =
                read_json(&reference(value.as_str().context("invalid reference")?)?)?;
        }
    }
    if let Some(expressions) = refs.get("Expressions") {
        for item in expressions.as_array().context("invalid Expressions")? {
            let path = reference(item["File"].as_str().context("missing expression File")?)?;
            mocari::expression::load_expression(path)?;
        }
    }
    if let Some(groups) = refs.get("Motions") {
        for items in groups.as_object().context("invalid Motions")?.values() {
            for item in items.as_array().context("invalid motion group")? {
                let _: serde_json::Value = read_json(&reference(
                    item["File"].as_str().context("missing motion File")?,
                )?)?;
                if item.get("Sound").is_some() {
                    anyhow::bail!("audio motions are not supported by pack v1");
                }
            }
        }
    }
    for action in [&manifest.actions.head_pat, &manifest.actions.body_tap]
        .into_iter()
        .flatten()
    {
        mocari::expression::load_expression(checked_path(&root, &action.expression)?)?;
    }
    Ok(Pack {
        root,
        manifest,
        entry,
    })
}
impl Pack {
    /// Used by the isolated host and import validator before making a pack active.
    pub fn load_model(&self) -> Result<mocari::assets::RuntimeModel> {
        let model = mocari::assets::load_model_runtime(&self.entry)?;
        let runtime = model.runtime();
        ensure!(runtime.meshes().len() <= 2048, "mesh count exceeds 2048");
        ensure!(
            runtime
                .meshes()
                .iter()
                .map(|m| m.vertices().len())
                .sum::<usize>()
                <= 500_000,
            "vertex budget exceeded"
        );
        for parameter in self.manifest.parameter_map.values() {
            ensure!(
                runtime.parameter_index(parameter).is_some(),
                "unknown mapped parameter: {parameter}"
            );
        }
        for action in [
            &self.manifest.actions.head_pat,
            &self.manifest.actions.body_tap,
        ]
        .into_iter()
        .flatten()
        {
            let expression =
                mocari::expression::load_expression(checked_path(&self.root, &action.expression)?)?;
            for duration in [
                expression.resolved_fade_in_time(),
                expression.resolved_fade_out_time(),
            ] {
                ensure!(
                    duration.is_finite() && (0.0..=2.0).contains(&duration),
                    "expression fade must be 0–2 seconds"
                );
            }
            for parameter in expression.parameters() {
                ensure!(
                    runtime.parameter_index(parameter.id()).is_some(),
                    "unknown expression parameter: {}",
                    parameter.id()
                );
                ensure!(parameter.value().is_finite(), "non-finite expression value");
            }
        }
        Ok(model)
    }
}
/// Copies to an isolated staging directory, validates there, then atomically
/// renames to a content-addressed destination. Original files are never modified.
pub fn import(source: &Path, library: &Path) -> Result<PathBuf> {
    let pack = open(source)?;
    fs::create_dir_all(library)?;
    ensure!(
        !library.canonicalize()?.starts_with(&pack.root),
        "library cannot be inside source pack"
    );
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let staging = library.join(format!(".import-{}-{nonce}", std::process::id()));
    fs::create_dir(&staging)?;
    let result = (|| -> Result<PathBuf> {
        let mut hash = Sha256::new();
        let mut copied = 0u64;
        for relative in inventory(&pack.root)? {
            let from = checked_path(&pack.root, relative.to_str().context("non-UTF8 pack path")?)?;
            let mut bytes = Vec::new();
            fs::File::open(from)?
                .take(32 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= 32 * 1024 * 1024, "asset grew during copy");
            copied += bytes.len() as u64;
            ensure!(copied <= MAX_BYTES, "pack grew during copy");
            hash.update(relative.to_string_lossy().as_bytes());
            hash.update([0]);
            hash.update((bytes.len() as u64).to_le_bytes());
            hash.update(&bytes);
            let to = staging.join(&relative);
            fs::create_dir_all(to.parent().unwrap())?;
            fs::write(to, bytes)?;
        }
        let validated = open(&staging.join("manifest.json"))?;
        // Structural and image checks precede isolated model parsing at activation.
        let digest = format!("{:x}", hash.finalize());
        let destination = library.join(format!("{}-{digest}", validated.manifest.id));
        if destination.exists() {
            open(&destination.join("manifest.json"))?;
            let mut existing = Sha256::new();
            for relative in inventory(&destination)? {
                let bytes = fs::read(destination.join(&relative))?;
                existing.update(relative.to_string_lossy().as_bytes());
                existing.update([0]);
                existing.update((bytes.len() as u64).to_le_bytes());
                existing.update(bytes);
            }
            ensure!(
                format!("{:x}", existing.finalize()) == digest,
                "existing imported pack was modified; reimport into a clean library"
            );
        } else {
            fs::rename(&staging, &destination)?;
        }
        Ok(destination.join("manifest.json"))
    })();
    let _ = fs::remove_dir_all(staging);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "avatar-pack-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir_all(root.join("source/model")).unwrap();
            let manifest: Manifest = serde_json::from_str(include_str!(
                "../../../assets/demo/pack-template/manifest.example.json"
            ))
            .unwrap();
            fs::write(
                root.join("source/manifest.json"),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
            fs::write(root.join("source/model/model.model3.json"),br#"{"Version":3,"FileReferences":{"Moc":"model.moc3","Textures":["texture.png"]}}"#).unwrap();
            fs::write(
                root.join("source/model/model.moc3"),
                b"deliberately invalid runtime fixture",
            )
            .unwrap();
            image::RgbaImage::new(1, 1)
                .save(root.join("source/model/texture.png"))
                .unwrap();
            Self(root)
        }
        fn manifest(&self) -> PathBuf {
            self.0.join("source/manifest.json")
        }
        fn edit(&self, change: impl FnOnce(&mut serde_json::Value)) {
            let mut value = read_json(&self.manifest()).unwrap();
            change(&mut value);
            fs::write(self.manifest(), serde_json::to_vec(&value).unwrap()).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn import_is_atomic_idempotent_and_preserves_source() {
        let f = Fixture::new();
        let before = fs::read(f.manifest()).unwrap();
        let first = import(&f.manifest(), &f.0.join("library")).unwrap();
        assert_eq!(first, import(&f.manifest(), &f.0.join("library")).unwrap());
        assert_eq!(fs::read(f.manifest()).unwrap(), before);
        assert_eq!(fs::read_dir(f.0.join("library")).unwrap().count(), 1);
        // Header validation intentionally precedes isolated runtime validation.
        assert!(open(&first).unwrap().load_model().is_err());
        fs::write(
            first.parent().unwrap().join("model/model.moc3"),
            b"tampered",
        )
        .unwrap();
        assert!(import(&f.manifest(), &f.0.join("library")).is_err());
    }
    #[test]
    fn unsupported_version_and_script_fields_rejected() {
        let f = Fixture::new();
        f.edit(|v| v["schema_version"] = 2.into());
        assert!(open(&f.manifest()).is_err());
        f.edit(|v| {
            v["schema_version"] = 1.into();
            v["script"] = "run.sh".into();
        });
        assert!(open(&f.manifest()).is_err());
    }
    #[test]
    fn traversal_and_missing_assets_rejected() {
        let f = Fixture::new();
        for entry in [
            "../outside.json",
            "/tmp/outside.json",
            "model/../model/model.model3.json",
            "model/missing.json",
        ] {
            f.edit(|v| v["entry"] = entry.into());
            assert!(open(&f.manifest()).is_err());
        }
    }
    #[cfg(unix)]
    #[test]
    fn symlink_and_executable_files_rejected() {
        let f = Fixture::new();
        std::os::unix::fs::symlink("model/model.moc3", f.0.join("source/link.moc3")).unwrap();
        assert!(open(&f.manifest()).is_err());
        fs::remove_file(f.0.join("source/link.moc3")).unwrap();
        fs::write(f.0.join("source/run.sh"), b"echo no").unwrap();
        assert!(open(&f.manifest()).is_err());
    }
    #[test]
    fn oversized_texture_and_json_rejected_before_decode() {
        let f = Fixture::new();
        image::RgbaImage::new(8193, 1)
            .save(f.0.join("source/model/texture.png"))
            .unwrap();
        assert!(open(&f.manifest()).is_err());
        let file = fs::File::create(f.0.join("source/huge.json")).unwrap();
        file.set_len(MAX_JSON + 1).unwrap();
        assert!(open(&f.manifest()).is_err());
    }
    #[test]
    fn hit_regions_have_scaled_equivalence_and_invalid_polygons_rejected() {
        let f = Fixture::new();
        let pack = open(&f.manifest()).unwrap();
        for scale in [0.5, 1.0, 1.5] {
            let x = 250.0 * scale;
            let y = 120.0 * scale;
            assert_eq!(
                pack.manifest
                    .interaction
                    .hit([x / (500.0 * scale), y / (600.0 * scale)]),
                Some(HitRegion::Head)
            );
        }
        assert_eq!(pack.manifest.interaction.hit([0.0, 0.0]), None);
        assert_eq!(
            pack.manifest.interaction.hit([0.5, 0.7]),
            Some(HitRegion::Body)
        );
        f.edit(|v| v["interaction"]["head"] = serde_json::json!([[0, 0], [1, 1], [0, 1], [1, 0]]));
        assert!(open(&f.manifest()).is_err());
    }
    #[test]
    fn library_inside_source_is_rejected_without_recursive_copy() {
        let f = Fixture::new();
        assert!(import(&f.manifest(), &f.0.join("source/library")).is_err());
    }
    #[test]
    fn invalid_duration_and_license_rejected() {
        let f = Fixture::new();
        f.edit(|v| {
            v["actions"]["head_pat"] =
                serde_json::json!({"expression":"anything.json","duration_ms":6000})
        });
        assert!(open(&f.manifest()).is_err());
        f.edit(|v| {
            v["actions"]["head_pat"] = serde_json::Value::Null;
            v["license"]["redistributable"] = true.into();
        });
        assert!(open(&f.manifest()).is_err());
    }
    #[test]
    fn hover_margin_is_logical_pixels_without_expanding_click_regions() {
        let f = Fixture::new();
        let pack = open(&f.manifest()).unwrap();
        let regions = &pack.manifest.interaction;
        assert_eq!(regions.hit([0.29, 0.2]), None);
        assert!(regions.near([0.29, 0.2], [500.0, 600.0], 6.0));
        assert!(!regions.near([0.29, 0.2], [1000.0, 1200.0], 6.0));
        assert!(!regions.near([0.29, 0.2], [500.0, 600.0], 0.0));
    }
}
