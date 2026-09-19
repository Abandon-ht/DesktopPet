use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};
// Check paths before handing the model to Mocari, whose loader joins paths directly.
pub fn checked_path(root: &Path, name: &str) -> Result<PathBuf> {
    ensure!(
        !Path::new(name).is_absolute(),
        "absolute asset path rejected: {name}"
    );
    let path = root
        .join(name)
        .canonicalize()
        .with_context(|| format!("missing asset: {name}"))?;
    ensure!(
        path.starts_with(root),
        "asset escapes model directory: {name}"
    );
    ensure!(path.is_file(), "asset is not a file: {name}");
    Ok(path)
}

pub fn preflight(entry: &Path) -> Result<(PathBuf, Value, Vec<PathBuf>)> {
    let entry = entry.canonicalize().context("model entry does not exist")?;
    let root = entry.parent().context("model entry has no parent")?;
    let model: Value = serde_json::from_slice(&fs::read(&entry)?)?;
    ensure!(model["Version"] == 3, "expected model3 Version 3");
    let refs = model["FileReferences"]
        .as_object()
        .context("missing FileReferences")?;
    let moc = refs
        .get("Moc")
        .and_then(Value::as_str)
        .context("missing Moc")?;
    checked_path(root, moc)?;
    let textures = refs
        .get("Textures")
        .and_then(Value::as_array)
        .context("missing Textures")?;
    ensure!(
        !textures.is_empty() && textures.len() <= 32,
        "expected 1–32 textures"
    );
    for item in textures {
        checked_path(
            root,
            item.as_str().context("texture path must be a string")?,
        )?;
    }
    for key in ["Physics", "Pose", "DisplayInfo", "UserData"] {
        if let Some(value) = refs.get(key) {
            let path = checked_path(root, value.as_str().context("asset path must be a string")?)?;
            let _: Value = serde_json::from_slice(&fs::read(&path)?)
                .with_context(|| path.display().to_string())?;
        }
    }
    let mut expressions = Vec::new();
    if let Some(items) = refs.get("Expressions") {
        for item in items.as_array().context("Expressions must be an array")? {
            expressions.push(checked_path(
                root,
                item["File"].as_str().context("missing expression File")?,
            )?);
        }
    }
    if let Some(groups) = refs.get("Motions") {
        for items in groups
            .as_object()
            .context("Motions must be an object")?
            .values()
        {
            for item in items.as_array().context("motion group must be an array")? {
                checked_path(root, item["File"].as_str().context("missing motion File")?)?;
                if let Some(sound) = item.get("Sound") {
                    checked_path(root, sound.as_str().context("Sound must be a string")?)?;
                }
            }
        }
    }
    // This local audit also discovers unregistered expressions beside the entry file.
    // It never modifies the user's model manifest.
    for item in fs::read_dir(root)? {
        let item = item?;
        let name = item.file_name();
        if name.to_string_lossy().ends_with(".exp3.json") {
            expressions.push(checked_path(
                root,
                name.to_str().context("non-UTF8 expression name")?,
            )?);
        }
    }
    expressions.sort();
    expressions.dedup();
    Ok((entry, model, expressions))
}
