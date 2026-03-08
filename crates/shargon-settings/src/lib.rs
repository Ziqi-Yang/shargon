use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, bail};
use serde::Deserialize;

const CONFIG_ENV_VAR: &str = "SHARGON_CONFIG";
const CONFIG_DIR_NAME: &str = "shargon";
const CONFIG_FILE_NAME: &str = "shargon.toml";
const DEFAULT_SOCKET_PATH: &str = "/tmp/shargon-daemon.sock";
const DEFAULT_MACHINE_PREFIX: &str = "shargon";

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ShargonSettings {
    pub daemon: DaemonSettings,
    pub backend: BackendSettings,
}

impl ShargonSettings {
    pub fn load() -> anyhow::Result<Self> {
        Self::load_with_sources(
            None,
            env::var_os(CONFIG_ENV_VAR).as_deref(),
            env::var_os("XDG_CONFIG_HOME").as_deref(),
            env::var_os("HOME").as_deref(),
        )
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read config from {}", path.display()))?;
        let settings = toml::from_str::<Self>(&contents)
            .with_context(|| format!("failed to parse config from {}", path.display()))?;
        settings
            .validate()
            .with_context(|| format!("invalid config at {}", path.display()))?;
        Ok(settings)
    }

    pub fn default_path() -> anyhow::Result<PathBuf> {
        resolve_default_path(
            env::var_os("XDG_CONFIG_HOME").as_deref(),
            env::var_os("HOME").as_deref(),
        )
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        self.daemon.validate().context("invalid daemon settings")?;
        self.backend.validate().context("invalid backend settings")
    }

    fn load_with_sources(
        explicit_path: Option<&Path>,
        config_env: Option<&OsStr>,
        xdg_config_home: Option<&OsStr>,
        home: Option<&OsStr>,
    ) -> anyhow::Result<Self> {
        if let Some(path) = explicit_path {
            return Self::load_from_path(path);
        }

        if let Some(path) = path_from_os_str(config_env) {
            return Self::load_from_path(path);
        }

        let default_path = match resolve_default_path(xdg_config_home, home) {
            Ok(path) => path,
            Err(_) => return Ok(Self::default()),
        };

        if !default_path.exists() {
            return Ok(Self::default());
        }

        Self::load_from_path(default_path)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct DaemonSettings {
    pub socket_path: PathBuf,
    #[serde(with = "humantime_serde")]
    pub readiness_timeout: Duration,
    #[serde(with = "humantime_serde")]
    pub retry_interval: Duration,
}

impl Default for DaemonSettings {
    fn default() -> Self {
        Self {
            socket_path: PathBuf::from(DEFAULT_SOCKET_PATH),
            readiness_timeout: Duration::from_secs(3),
            retry_interval: Duration::from_millis(50),
        }
    }
}

impl DaemonSettings {
    pub fn validate(&self) -> anyhow::Result<()> {
        if !self.socket_path.is_absolute() {
            bail!(
                "socket_path must be absolute, got {}",
                self.socket_path.display()
            );
        }

        if self.readiness_timeout.is_zero() {
            bail!("readiness_timeout must be greater than zero");
        }

        if self.retry_interval.is_zero() {
            bail!("retry_interval must be greater than zero");
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct BackendSettings {
    pub default: BackendKind,
    pub default_parallel_vms: usize,
    pub nspawn: NspawnSettings,
}

impl Default for BackendSettings {
    fn default() -> Self {
        Self {
            default: BackendKind::Nspawn,
            default_parallel_vms: 1,
            nspawn: NspawnSettings::default(),
        }
    }
}

impl BackendSettings {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.default_parallel_vms == 0 {
            bail!("default_parallel_vms must be greater than zero");
        }

        match self.default {
            BackendKind::Nspawn => self.nspawn.validate(),
            BackendKind::Qemu => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    Nspawn,
    Qemu,
}

impl Default for BackendKind {
    fn default() -> Self {
        Self::Nspawn
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct NspawnSettings {
    pub root_directory: Option<PathBuf>,
    pub machine_prefix: String,
    #[serde(with = "humantime_serde")]
    pub boot_timeout: Duration,
}

impl Default for NspawnSettings {
    fn default() -> Self {
        Self {
            root_directory: None,
            machine_prefix: DEFAULT_MACHINE_PREFIX.to_string(),
            boot_timeout: Duration::from_secs(30),
        }
    }
}

impl NspawnSettings {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.machine_prefix.is_empty() {
            bail!("machine_prefix must not be empty");
        }

        if self.boot_timeout.is_zero() {
            bail!("boot_timeout must be greater than zero");
        }

        if let Some(root_directory) = &self.root_directory {
            if !root_directory.is_absolute() {
                bail!(
                    "root_directory must be absolute, got {}",
                    root_directory.display()
                );
            }

            if !root_directory.exists() {
                bail!(
                    "root_directory does not exist: {}",
                    root_directory.display()
                );
            }

            if !root_directory.is_dir() {
                bail!(
                    "root_directory must be a directory, got {}",
                    root_directory.display()
                );
            }
        }

        Ok(())
    }
}

fn resolve_default_path(
    xdg_config_home: Option<&OsStr>,
    home: Option<&OsStr>,
) -> anyhow::Result<PathBuf> {
    if let Some(path) = path_from_os_str(xdg_config_home) {
        return Ok(path.join(CONFIG_DIR_NAME).join(CONFIG_FILE_NAME));
    }

    if let Some(path) = path_from_os_str(home) {
        return Ok(path
            .join(".config")
            .join(CONFIG_DIR_NAME)
            .join(CONFIG_FILE_NAME));
    }

    bail!("unable to resolve default config path from XDG_CONFIG_HOME or HOME")
}

fn path_from_os_str(value: Option<&OsStr>) -> Option<PathBuf> {
    value.filter(|value| !value.is_empty()).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn default_settings_match_expected_values() {
        let settings = ShargonSettings::default();

        assert_eq!(
            settings.daemon.socket_path,
            PathBuf::from(DEFAULT_SOCKET_PATH)
        );
        assert_eq!(settings.daemon.readiness_timeout, Duration::from_secs(3));
        assert_eq!(settings.daemon.retry_interval, Duration::from_millis(50));
        assert_eq!(settings.backend.default, BackendKind::Nspawn);
        assert_eq!(settings.backend.default_parallel_vms, 1);
        assert_eq!(settings.backend.nspawn.root_directory, None);
        assert_eq!(settings.backend.nspawn.machine_prefix, DEFAULT_MACHINE_PREFIX);
        assert_eq!(settings.backend.nspawn.boot_timeout, Duration::from_secs(30));
    }

    #[test]
    fn parses_valid_toml() -> anyhow::Result<()> {
        let temp_dir = tempdir()?;
        let root_directory = temp_dir.path().join("rootfs");
        fs::create_dir(&root_directory)?;
        let config_path = temp_dir.path().join("shargon.toml");

        fs::write(
            &config_path,
            format!(
                r#"
[daemon]
socket_path = "/tmp/custom-shargon.sock"
readiness_timeout = "5s"
retry_interval = "125ms"

[backend]
default = "nspawn"
default_parallel_vms = 4

[backend.nspawn]
root_directory = "{}"
machine_prefix = "ci"
boot_timeout = "45s"
"#,
                root_directory.display()
            ),
        )?;

        let settings = ShargonSettings::load_from_path(&config_path)?;

        assert_eq!(
            settings.daemon,
            DaemonSettings {
                socket_path: PathBuf::from("/tmp/custom-shargon.sock"),
                readiness_timeout: Duration::from_secs(5),
                retry_interval: Duration::from_millis(125),
            }
        );
        assert_eq!(
            settings.backend,
            BackendSettings {
                default: BackendKind::Nspawn,
                default_parallel_vms: 4,
                nspawn: NspawnSettings {
                    root_directory: Some(root_directory),
                    machine_prefix: "ci".to_string(),
                    boot_timeout: Duration::from_secs(45),
                },
            }
        );
        Ok(())
    }

    #[test]
    fn load_returns_defaults_when_default_path_is_missing() -> anyhow::Result<()> {
        let temp_dir = tempdir()?;
        let xdg_home = temp_dir.path().join("xdg");

        let settings =
            ShargonSettings::load_with_sources(None, None, Some(xdg_home.as_os_str()), None)?;

        assert_eq!(settings, ShargonSettings::default());
        Ok(())
    }

    #[test]
    fn default_path_prefers_xdg_config_home() -> anyhow::Result<()> {
        let temp_dir = tempdir()?;
        let xdg_home = temp_dir.path().join("xdg");

        let path = resolve_default_path(Some(xdg_home.as_os_str()), None)?;

        assert_eq!(path, xdg_home.join("shargon").join("shargon.toml"));
        Ok(())
    }

    #[test]
    fn default_path_falls_back_to_home_config_dir() -> anyhow::Result<()> {
        let temp_dir = tempdir()?;
        let home = temp_dir.path().join("home");

        let path = resolve_default_path(None, Some(home.as_os_str()))?;

        assert_eq!(
            path,
            home.join(".config").join("shargon").join("shargon.toml")
        );
        Ok(())
    }

    #[test]
    fn explicit_missing_path_returns_error() {
        let err = ShargonSettings::load_from_path("/definitely/missing/shargon.toml").unwrap_err();
        assert!(format!("{err:#}").contains("failed to read config"));
    }

    #[test]
    fn invalid_toml_returns_path_context() -> anyhow::Result<()> {
        let temp_dir = tempdir()?;
        let config_path = temp_dir.path().join("shargon.toml");
        fs::write(&config_path, "[daemon]\nretry_interval = [")?;

        let err = ShargonSettings::load_from_path(&config_path).unwrap_err();
        let rendered = format!("{err:#}");

        assert!(rendered.contains("failed to parse config"));
        assert!(rendered.contains(&config_path.display().to_string()));
        Ok(())
    }

    #[test]
    fn validate_rejects_relative_socket_paths() {
        let err = ShargonSettings {
            daemon: DaemonSettings {
                socket_path: PathBuf::from("relative.sock"),
                ..DaemonSettings::default()
            },
            ..test_settings()
        }
        .validate()
        .unwrap_err();

        assert!(format!("{err:#}").contains("socket_path must be absolute"));
    }

    #[test]
    fn validate_rejects_zero_durations() {
        let err = ShargonSettings {
            daemon: DaemonSettings {
                readiness_timeout: Duration::ZERO,
                ..DaemonSettings::default()
            },
            ..test_settings()
        }
        .validate()
        .unwrap_err();

        assert!(format!("{err:#}").contains("readiness_timeout"));

        let err = ShargonSettings {
            daemon: DaemonSettings {
                retry_interval: Duration::ZERO,
                ..DaemonSettings::default()
            },
            ..test_settings()
        }
        .validate()
        .unwrap_err();

        assert!(format!("{err:#}").contains("retry_interval"));
    }

    #[test]
    fn validate_rejects_zero_parallelism() {
        let err = ShargonSettings {
            backend: BackendSettings {
                default_parallel_vms: 0,
                ..test_settings().backend
            },
            ..test_settings()
        }
        .validate()
        .unwrap_err();

        assert!(format!("{err:#}").contains("default_parallel_vms"));
    }

    #[test]
    fn validate_rejects_relative_root_directory() {
        let err = ShargonSettings {
            backend: BackendSettings {
                nspawn: NspawnSettings {
                    root_directory: Some(PathBuf::from("relative")),
                    ..test_settings().backend.nspawn
                },
                ..test_settings().backend
            },
            ..test_settings()
        }
        .validate()
        .unwrap_err();

        assert!(format!("{err:#}").contains("root_directory must be absolute"));
    }

    #[test]
    fn validate_rejects_missing_root_directory() {
        let temp_dir = tempdir().expect("temp dir");
        let missing_root = temp_dir.path().join("missing");

        let err = ShargonSettings {
            backend: BackendSettings {
                nspawn: NspawnSettings {
                    root_directory: Some(missing_root.clone()),
                    ..test_settings().backend.nspawn
                },
                ..test_settings().backend
            },
            ..test_settings()
        }
        .validate()
        .unwrap_err();

        assert!(format!("{err:#}").contains(&missing_root.display().to_string()));
    }

    #[test]
    fn unknown_fields_are_rejected() -> anyhow::Result<()> {
        let temp_dir = tempdir()?;
        let root_directory = temp_dir.path().join("rootfs");
        fs::create_dir(&root_directory)?;
        let config_path = temp_dir.path().join("shargon.toml");

        fs::write(
            &config_path,
            format!(
                r#"
[backend]
default_parallel_vms = 2
unknown = "value"

[backend.nspawn]
root_directory = "{}"
"#,
                root_directory.display()
            ),
        )?;

        let err = ShargonSettings::load_from_path(&config_path).unwrap_err();
        assert!(format!("{err:#}").contains("unknown field"));
        Ok(())
    }

    fn test_settings() -> ShargonSettings {
        let root_directory = tempdir().expect("temp dir");
        let root_directory = root_directory.keep();

        ShargonSettings {
            daemon: DaemonSettings::default(),
            backend: BackendSettings {
                default: BackendKind::Nspawn,
                default_parallel_vms: 1,
                nspawn: NspawnSettings {
                    root_directory: Some(root_directory),
                    machine_prefix: DEFAULT_MACHINE_PREFIX.to_string(),
                    boot_timeout: Duration::from_secs(30),
                },
            },
        }
    }
}
