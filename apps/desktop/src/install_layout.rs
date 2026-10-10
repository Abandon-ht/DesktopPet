//! Executables and resource roots for the macOS bundle and Linux portable tree.
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub fn is_ax650() -> bool {
    std::env::var_os("DESKTOPPET_DEVICE_PROFILE").is_some_and(|profile| profile == "ax650")
}

pub fn initial_scale() -> Result<u16> {
    match std::env::var("DESKTOPPET_INITIAL_SCALE") {
        Ok(value) => {
            let scale = value
                .parse::<u16>()
                .context("invalid DESKTOPPET_INITIAL_SCALE")?;
            anyhow::ensure!(
                (50..=150).contains(&scale),
                "DESKTOPPET_INITIAL_SCALE must be 50–150"
            );
            Ok(scale)
        }
        Err(std::env::VarError::NotPresent) => Ok(if is_ax650() { 64 } else { 100 }),
        Err(error) => Err(error).context("invalid DESKTOPPET_INITIAL_SCALE"),
    }
}

pub fn data_directory(
    override_directory: Option<PathBuf>,
    development_config: &Path,
    fallback: impl FnOnce() -> Result<PathBuf>,
) -> Result<PathBuf> {
    if let Some(directory) = override_directory {
        anyhow::ensure!(
            directory.is_absolute(),
            "DESKTOPPET_DATA_DIR must be an absolute path"
        );
        return Ok(directory);
    }
    if development_config.is_file() {
        let configured = std::fs::read_to_string(development_config)?;
        anyhow::ensure!(!configured.trim().is_empty(), "dev-data-dir.txt is empty");
        return Ok(PathBuf::from(configured.trim()));
    }
    fallback()
}

pub struct Layout {
    pub host: PathBuf,
    pub resources: PathBuf,
}
impl Layout {
    pub fn current(executable: &Path) -> Result<Self> {
        let directory = executable
            .parent()
            .context("missing executable directory")?;
        #[cfg(target_os = "linux")]
        return Ok(Self::portable(
            directory,
            std::env::var_os("DESKTOPPET_RESOURCE_DIR").map(PathBuf::from),
        ));
        #[cfg(not(target_os = "linux"))]
        {
            let contents = directory
                .parent()
                .context("missing app contents directory")?;
            let bundled =
                contents.join("Helpers/DesktopPet Avatar Host.app/Contents/MacOS/avatar-host-2d");
            Ok(Self {
                host: if bundled.is_file() {
                    bundled
                } else {
                    directory.join("avatar-host-2d")
                },
                resources: contents.join("Resources"),
            })
        }
    }
    #[cfg(any(target_os = "linux", test))]
    fn portable(directory: &Path, override_resources: Option<PathBuf>) -> Self {
        Self {
            host: directory.join("avatar-host-2d"),
            resources: override_resources.unwrap_or_else(|| directory.join("resources")),
        }
    }
    pub fn model(&self) -> Result<Option<PathBuf>> {
        if let Some(model) = std::env::var_os("DESKTOPPET_MODEL") {
            return Ok(Some(PathBuf::from(model)));
        }
        let config = self.resources.join("model-path.txt");
        if !config.is_file() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(config)?;
        let configured = PathBuf::from(text.trim());
        anyhow::ensure!(
            !configured.as_os_str().is_empty(),
            "model-path.txt is empty"
        );
        Ok(Some(if configured.is_absolute() {
            configured
        } else {
            self.resources.join(configured)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_data_directory_takes_precedence_over_portable_config() {
        let directory = data_directory(
            Some("/opt/desktop-pet/data".into()),
            Path::new("missing-development-config"),
            || panic!("explicit data directory must not use app defaults"),
        )
        .unwrap();
        assert_eq!(directory, Path::new("/opt/desktop-pet/data"));
        assert!(
            data_directory(
                Some("relative-data".into()),
                Path::new("missing"),
                || unreachable!()
            )
            .is_err()
        );
    }
    #[test]
    fn portable_layout_moves_resources_with_binaries_and_accepts_external_resources() {
        let layout = Layout::portable(Path::new("/opt/Desktop Pet"), None);
        assert_eq!(layout.host, Path::new("/opt/Desktop Pet/avatar-host-2d"));
        assert_eq!(layout.resources, Path::new("/opt/Desktop Pet/resources"));
        let moved = Layout::portable(
            Path::new("/home/user/Desktop Pet"),
            Some("/mnt/assets".into()),
        );
        assert_eq!(
            moved.host,
            Path::new("/home/user/Desktop Pet/avatar-host-2d")
        );
        assert_eq!(moved.resources, Path::new("/mnt/assets"));
    }
}
