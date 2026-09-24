use super::*;
use serde::{Deserialize, Serialize};
#[derive(Serialize)]
pub struct PackItem {
    path: String,
    name: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    selected: PathBuf,
    scale: u16,
}
#[derive(Serialize)]
pub struct Preferences {
    scale: u16,
    importing: bool,
    error: Option<String>,
}
fn directory(shared: &Shared) -> Result<PathBuf, String> {
    shared
        .library
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "角色库尚未初始化".into())
}
pub(super) fn legacy_model_upgrade(
    selected: &std::path::Path,
    bundled: Option<&std::path::Path>,
    import: impl FnOnce(&std::path::Path) -> Result<PathBuf>,
) -> Result<Option<PathBuf>> {
    let is_raw_model = selected
        .file_name()
        .is_some_and(|name| name.to_string_lossy().ends_with(".model3.json"));
    let Some(bundled) =
        bundled.filter(|path| path.file_name().is_some_and(|n| n == "manifest.json"))
    else {
        return Ok(None);
    };
    if is_raw_model {
        return import(bundled).map(Some);
    }
    Ok(None)
}
pub fn restore(shared: &Shared, directory: &std::path::Path, bundled: Option<&std::path::Path>) {
    if let Ok(saved) = avatar_pack::read_json::<Saved>(&directory.join("preferences.json"))
        && saved.version == 1
        && (50..=150).contains(&saved.scale)
        && saved.selected.is_file()
    {
        shared.scale.store(saved.scale, Ordering::Release);
        let selected = match legacy_model_upgrade(&saved.selected, bundled, |path| {
            avatar_pack::import(path, &directory.join("packs"))
        }) {
            Ok(Some(pack)) => {
                pet_ipc::event_log!(
                    "{}",
                    serde_json::json!({"event":"legacy_model_upgraded","pack":pack})
                );
                save_selection(shared, &pack);
                pack
            }
            Ok(None) => saved.selected,
            Err(error) => {
                *shared.import_error.lock().unwrap() = Some(format!(
                    "旧模型尚未升级为角色包：{error:#}。可在“角色与设置”中手动导入。"
                ));
                saved.selected
            }
        };
        *shared.requested.lock().unwrap() = Some(selected);
    }
}
pub fn save_selection(shared: &Shared, path: &std::path::Path) {
    let Ok(directory) = directory(shared) else {
        return;
    };
    let save = (|| -> Result<()> {
        let saved = Saved {
            version: 1,
            selected: path.to_owned(),
            scale: shared.scale.load(Ordering::Acquire),
        };
        let temp = directory.join("preferences.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(&saved)?)?;
        std::fs::rename(temp, directory.join("preferences.json"))?;
        Ok(())
    })();
    if let Err(error) = save {
        *shared.import_error.lock().unwrap() = Some(format!("无法保存设置：{error:#}"));
    }
}
#[tauri::command]
pub fn preferences(state: tauri::State<'_, Arc<Shared>>) -> Preferences {
    Preferences {
        scale: state.scale.load(Ordering::Acquire),
        importing: state.importing.load(Ordering::Acquire),
        error: state.import_error.lock().unwrap().clone(),
    }
}
#[tauri::command]
pub fn packs(state: tauri::State<'_, Arc<Shared>>) -> Result<Vec<PackItem>, String> {
    let library = directory(&state)?.join("packs");
    if !library.exists() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    for item in std::fs::read_dir(library).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        if item.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = item.path().join("manifest.json");
        if let Ok(manifest) = avatar_pack::read_json::<avatar_pack::Manifest>(&path) {
            items.push(PackItem {
                path: path.to_string_lossy().into_owned(),
                name: manifest.display_name,
            });
        }
    }
    items.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(items)
}
fn choose(shared: &Shared, path: PathBuf) {
    *shared.import_error.lock().unwrap() = None;
    *shared.requested.lock().unwrap() = Some(path);
    shared.selection.fetch_add(1, Ordering::AcqRel);
    let _ = shared.wake.try_send(());
}
#[tauri::command]
pub fn select_pack(path: String, state: tauri::State<'_, Arc<Shared>>) -> Result<(), String> {
    let library = directory(&state)?
        .join("packs")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let path = PathBuf::from(path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !path.starts_with(library) || path.file_name().is_none_or(|n| n != "manifest.json") {
        return Err("请选择已导入的角色包".into());
    }
    choose(&state, path);
    Ok(())
}
#[tauri::command]
pub async fn import_pack(
    path: String,
    state: tauri::State<'_, Arc<Shared>>,
) -> Result<String, String> {
    let shared = state.inner().clone();
    if shared.importing.swap(true, Ordering::AcqRel) {
        return Err("已有导入正在进行".into());
    }
    let worker = shared.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let library = directory(&worker)?.join("packs");
        let path = avatar_pack::import(std::path::Path::new(&path), &library)
            .map_err(|e| format!("{e:#}"))?;
        choose(&worker, path.clone());
        Ok(path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    shared.importing.store(false, Ordering::Release);
    if let Err(error) = &result {
        *shared.import_error.lock().unwrap() = Some(error.clone());
    }
    result
}
#[tauri::command]
pub fn set_scale(scale: u16, state: tauri::State<'_, Arc<Shared>>) -> Result<(), String> {
    if !(50..=150).contains(&scale) {
        return Err("大小范围为 50%–150%".into());
    }
    state.scale.store(scale, Ordering::Release);
    let _ = state.wake.try_send(());
    Ok(())
}
