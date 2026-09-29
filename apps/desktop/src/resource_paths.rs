//! Store bundled asset paths independently of the app's installation directory.

use pet_voice_session::VoiceSettings;
use std::path::{Path, PathBuf};

const BUNDLE_PREFIX: &str = "@desktop-pet-resources";

fn paths(settings: &mut VoiceSettings) -> [&mut PathBuf; 12] {
    [
        &mut settings.model_dir,
        &mut settings.asr_model_path,
        &mut settings.asr_ncnn_model_dir,
        &mut settings.asr_ncnn_executable,
        &mut settings.asr_mlx_model_dir,
        &mut settings.tts_model_dir,
        &mut settings.kokoro_model_dir,
        &mut settings.vad_model_path,
        &mut settings.kws_model_dir,
        &mut settings.kws_keywords_file,
        &mut settings.greeting_dir,
        &mut settings.reference_audio,
    ]
}

/// Convert paths within the current app's Resources folder to portable saved values.
pub fn for_storage(settings: &VoiceSettings, resources: &Path) -> VoiceSettings {
    let mut saved = settings.clone();
    for path in paths(&mut saved) {
        *path = for_storage_path(path, resources);
    }
    saved
}

pub fn for_storage_path(path: &Path, resources: &Path) -> PathBuf {
    if let Ok(relative) = path.strip_prefix(resources)
        && !relative.as_os_str().is_empty()
    {
        return Path::new(BUNDLE_PREFIX).join(relative);
    }
    path.to_owned()
}

/// Expand paths saved relative to the current app's Resources folder.
pub fn for_runtime(settings: &mut VoiceSettings, resources: &Path) {
    for path in paths(settings) {
        *path = for_runtime_path(path, resources);
    }
}

pub fn for_runtime_path(path: &Path, resources: &Path) -> PathBuf {
    if let Ok(relative) = path.strip_prefix(BUNDLE_PREFIX)
        && !relative.as_os_str().is_empty()
    {
        return resources.join(relative);
    }
    path.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn bundled_paths_survive_app_move_and_custom_paths_stay_untouched() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "desktop-pet-resources-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let old_app = root.join("Downloads/DesktopPet.app");
        let new_app = root.join("Applications/DesktopPet.app");
        let old_resources = old_app.join("Contents/Resources");
        let new_resources = new_app.join("Contents/Resources");
        std::fs::create_dir_all(old_resources.join("models/kws")).unwrap();
        std::fs::create_dir_all(old_resources.join("voice")).unwrap();
        std::fs::create_dir_all(old_resources.join("avatar")).unwrap();
        std::fs::write(old_resources.join("voice/reference.wav"), []).unwrap();
        std::fs::write(old_resources.join("avatar/manifest.json"), b"{}").unwrap();
        let custom = root.join("custom/model.onnx");
        let original = VoiceSettings {
            model_dir: old_resources.join("models"),
            kws_model_dir: old_resources.join("models/kws"),
            greeting_dir: old_resources.join("voice"),
            reference_audio: old_resources.join("voice/reference.wav"),
            asr_model_path: custom.clone(),
            ..VoiceSettings::default()
        };
        let portable = for_storage(&original, &old_resources);
        let portable_avatar =
            for_storage_path(&old_resources.join("avatar/manifest.json"), &old_resources);
        assert_eq!(portable.model_dir, Path::new(BUNDLE_PREFIX).join("models"));
        assert_eq!(portable.asr_model_path, custom);
        std::fs::create_dir_all(new_app.parent().unwrap()).unwrap();
        std::fs::rename(&old_app, &new_app).unwrap();
        let mut loaded: VoiceSettings =
            serde_json::from_str(&serde_json::to_string(&portable).unwrap()).unwrap();
        for_runtime(&mut loaded, &new_resources);
        assert_eq!(loaded.model_dir, new_resources.join("models"));
        assert_eq!(loaded.kws_model_dir, new_resources.join("models/kws"));
        assert_eq!(loaded.greeting_dir, new_resources.join("voice"));
        assert_eq!(
            loaded.reference_audio,
            new_resources.join("voice/reference.wav")
        );
        assert_eq!(loaded.asr_model_path, custom);
        assert_eq!(
            for_runtime_path(&portable_avatar, &new_resources),
            new_resources.join("avatar/manifest.json")
        );

        std::fs::remove_dir_all(root).unwrap();
    }
}
