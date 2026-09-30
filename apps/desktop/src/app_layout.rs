//! Installation paths are relative to the executable, never the launch directory.
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub struct Layout {
    pub host: PathBuf,
    pub resources: PathBuf,
}

impl Layout {
    pub fn discover() -> Result<Self> {
        let mut layout = Self::beside(&std::env::current_exe()?)?;
        if let Some(resources) = std::env::var_os("DESKTOPPET_RESOURCES") {
            layout.resources = PathBuf::from(resources)
                .canonicalize()
                .context("DESKTOPPET_RESOURCES directory does not exist")?;
            anyhow::ensure!(
                layout.resources.is_dir(),
                "DESKTOPPET_RESOURCES must be a directory"
            );
        }
        Ok(layout)
    }

    fn beside(executable: &Path) -> Result<Self> {
        let directory = executable
            .parent()
            .context("missing executable directory")?;
        #[cfg(target_os = "macos")]
        {
            let contents = directory
                .parent()
                .context("missing app contents directory")?;
            let helper =
                contents.join("Helpers/DesktopPet Avatar Host.app/Contents/MacOS/avatar-host-2d");
            Ok(Self {
                host: if helper.is_file() {
                    helper
                } else {
                    directory.join("avatar-host-2d")
                },
                resources: contents.join("Resources"),
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let resources = directory.join("resources");
            Ok(Self {
                host: directory.join(format!("avatar-host-2d{}", std::env::consts::EXE_SUFFIX)),
                resources: if resources.is_dir() {
                    resources
                } else {
                    directory.to_owned()
                },
            })
        }
    }

    pub fn model(&self) -> Result<Option<PathBuf>> {
        if let Some(path) = std::env::var_os("DESKTOPPET_MODEL") {
            return Ok(Some(PathBuf::from(path)));
        }
        self.bundled_model()
    }

    fn bundled_model(&self) -> Result<Option<PathBuf>> {
        let config = self.resources.join("model-path.txt");
        if config.is_file() {
            let text = std::fs::read_to_string(config)?;
            let path = PathBuf::from(text.trim().trim_start_matches('\u{feff}'));
            anyhow::ensure!(!path.as_os_str().is_empty(), "model-path.txt is empty");
            return Ok(Some(if path.is_absolute() {
                path
            } else {
                self.resources.join(path)
            }));
        }
        let manifest = self.resources.join("avatar/manifest.json");
        Ok(manifest.is_file().then_some(manifest))
    }
}

#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn portable_layout_moves_with_resources_and_finds_exe_suffix() {
        let root = std::env::temp_dir().join(format!("desktop-pet-layout-{}", std::process::id()));
        let old = root.join("old install");
        let new = root.join("new install");
        std::fs::create_dir_all(old.join("resources/avatar")).unwrap();
        std::fs::write(old.join("resources/avatar/manifest.json"), b"{}").unwrap();
        let layout = Layout::beside(&old.join("desktop-pet.exe")).unwrap();
        assert_eq!(
            layout.host,
            old.join(format!("avatar-host-2d{}", std::env::consts::EXE_SUFFIX))
        );
        assert_eq!(
            layout.bundled_model().unwrap(),
            Some(old.join("resources/avatar/manifest.json"))
        );
        std::fs::write(
            old.join("resources/model-path.txt"),
            "\u{feff}avatar/manifest.json\r\n",
        )
        .unwrap();
        std::fs::rename(&old, &new).unwrap();
        let moved = Layout::beside(&new.join("desktop-pet.exe")).unwrap();
        assert_eq!(
            moved.bundled_model().unwrap(),
            Some(new.join("resources/avatar/manifest.json"))
        );
        std::fs::write(new.join("resources/model-path.txt"), "\n").unwrap();
        assert!(moved.bundled_model().is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
