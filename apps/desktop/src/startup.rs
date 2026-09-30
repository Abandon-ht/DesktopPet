use serde::Serialize;

#[derive(Serialize)]
pub struct StartupStatus {
    supported: bool,
    enabled: bool,
}

#[tauri::command]
pub fn startup_status() -> Result<StartupStatus, String> {
    #[cfg(target_os = "windows")]
    {
        Ok(StartupStatus {
            supported: true,
            enabled: windows::enabled().map_err(|error| error.to_string())?,
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(StartupStatus {
            supported: false,
            enabled: false,
        })
    }
}

#[tauri::command]
pub fn set_startup_enabled(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        windows::set_enabled(enabled).map_err(|error| error.to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = enabled;
        Err("当前平台暂不支持此开机自启设置".into())
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use std::{io, path::Path};
    use winreg::{
        RegKey,
        enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE},
    };

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "DesktopPet";

    fn command(executable: &Path) -> io::Result<String> {
        let path = executable
            .to_str()
            .ok_or_else(|| io::Error::other("启动路径不是有效 Unicode"))?;
        if path.contains(['"', '\r', '\n']) {
            return Err(io::Error::other("启动路径包含无效字符"));
        }
        Ok(format!("\"{path}\""))
    }

    pub fn enabled() -> io::Result<bool> {
        let key = match RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, KEY_READ)
        {
            Ok(key) => key,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        enabled_at(&key, &std::env::current_exe()?)
    }

    fn enabled_at(key: &RegKey, executable: &Path) -> io::Result<bool> {
        match key.get_value::<String, _>(VALUE) {
            Ok(value) => Ok(value.eq_ignore_ascii_case(&command(executable)?)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub fn set_enabled(enabled: bool) -> io::Result<()> {
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey_with_flags(RUN_KEY, KEY_READ | KEY_WRITE)?;
        set_enabled_at(&key, &std::env::current_exe()?, enabled)
    }

    fn set_enabled_at(key: &RegKey, executable: &Path, enabled: bool) -> io::Result<()> {
        if enabled {
            key.set_value(VALUE, &command(executable)?)
        } else {
            match key.delete_value(VALUE) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn startup_roundtrip_quotes_unicode_paths_and_preserves_other_entries() {
            let root = RegKey::predef(HKEY_CURRENT_USER);
            let name = format!(r"Software\DesktopPet\Tests\Startup-{}", std::process::id());
            let (key, _) = root.create_subkey(&name).unwrap();
            let result = std::panic::catch_unwind(|| {
                let executable = Path::new(r"C:\测试目录\Desktop Pet\desktop-pet.exe");
                key.set_value("OtherApp", &"keep this entry").unwrap();
                assert!(!enabled_at(&key, executable).unwrap());
                set_enabled_at(&key, executable, true).unwrap();
                assert_eq!(
                    key.get_value::<String, _>(VALUE).unwrap(),
                    format!("\"{}\"", executable.display())
                );
                assert!(enabled_at(&key, executable).unwrap());
                assert!(!enabled_at(&key, Path::new(r"C:\other\desktop-pet.exe")).unwrap());
                set_enabled_at(&key, executable, false).unwrap();
                set_enabled_at(&key, executable, false).unwrap();
                assert!(!enabled_at(&key, executable).unwrap());
                assert_eq!(
                    key.get_value::<String, _>("OtherApp").unwrap(),
                    "keep this entry"
                );
            });
            drop(key);
            root.delete_subkey_all(&name).unwrap();
            result.unwrap();
        }
    }
}
