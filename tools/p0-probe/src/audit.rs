use anyhow::{Context, Result, bail, ensure};
use mocari::{
    assets::load_model_runtime,
    expression::{ExpressionPlayer, load_expression},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
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

pub fn audit(entry: &Path) -> Result<Value> {
    let (entry, manifest, expressions) = preflight(entry)?;
    let started = Instant::now();
    let mut model = load_model_runtime(&entry).context("Mocari 0.3.1 model load failed")?;
    let load_ms = started.elapsed().as_secs_f64() * 1000.0;
    let textures: Vec<_> = model
        .textures()
        .iter()
        .map(|t| json!({"width": t.width(), "height": t.height(), "decoded_bytes": t.rgba().len()}))
        .collect();
    let runtime = model.runtime_mut();
    let parameters: Vec<_> = runtime.parameter_infos().map(|p| json!({"id": p.id(), "min": p.minimum(), "max": p.maximum(), "default": p.default()})).collect();
    let mut parameter_checks = Vec::new();
    for id in ["ParamEyeLOpen", "ParamEyeROpen", "ParamMouthOpenY"] {
        let info = runtime
            .parameter_info(id)
            .with_context(|| format!("required parameter absent: {id}"))?;
        let (min, max) = (info.minimum(), info.maximum());
        for value in [min, max] {
            runtime.reset_parameters();
            ensure!(runtime.set_parameter(id, value), "cannot set {id}");
            runtime.update_meshes().context("mesh update failed")?;
            ensure_finite(runtime)?;
            parameter_checks.push(
                json!({"id": id, "value": value, "mesh_update": "passed", "visual": "pending"}),
            );
        }
    }
    let mut expression_checks = Vec::new();
    for path in &expressions {
        let expression = load_expression(path)
            .with_context(|| format!("invalid expression {}", path.display()))?;
        let unknown: Vec<_> = expression
            .parameters()
            .iter()
            .filter(|p| runtime.parameter_index(p.id()).is_none())
            .map(|p| p.id().to_owned())
            .collect();
        let mapping: Value = serde_json::from_slice(&fs::read(path)?)?;
        runtime.reset_parameters();
        let mut player = ExpressionPlayer::new(expression);
        player.tick(player.expression().resolved_fade_in_time() + 1.0);
        player.apply(runtime);
        runtime
            .update_meshes()
            .context("expression mesh update failed")?;
        ensure_finite(runtime)?;
        expression_checks.push(json!({"file": path.file_name().map(|s| s.to_string_lossy()), "parameters": mapping["Parameters"], "unknown_parameters": unknown, "mesh_update": "passed", "visual": "pending"}));
    }
    runtime.reset_parameters();
    let mut samples = Vec::new();
    for _ in 0..300 {
        let start = Instant::now();
        runtime.update_meshes().context("mesh update failed")?;
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    Ok(json!({
        "schema_version": 1, "mocari": "0.3.1", "entry": entry,
        "load_ms": load_ms, "load_scope": "Mocari CPU parsing, texture decode, initial meshes; excludes GPU and preflight",
        "parameters": parameters, "textures": textures,
        "drawables": runtime.meshes().len(),
        "masked_drawables": runtime.meshes().iter().filter(|m| !m.masks().is_empty()).count(),
        "expressions_registered_in_source": manifest["FileReferences"]["Expressions"].as_array().map_or(0, Vec::len),
        "hit_areas": manifest["HitAreas"].as_array().map_or(0, Vec::len),
        "parameter_checks": parameter_checks, "expressions": expression_checks,
        "cpu_mesh_update_ms": {"samples": 300, "p50": samples[149], "p95": samples[284], "max": samples[299]},
        "limitations": ["not a rendered-frame benchmark", "no visual compatibility verdict", "physics not evaluated", "not a 30-minute stability test"]
    }))
}

fn ensure_finite(runtime: &mocari::ModelRuntime) -> Result<()> {
    for mesh in runtime.meshes() {
        for vertex in mesh.vertices() {
            if !vertex.position().iter().all(|v| v.is_finite()) {
                bail!("non-finite mesh position");
            }
        }
    }
    Ok(())
}
