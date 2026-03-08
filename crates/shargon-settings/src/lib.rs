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

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ShargonSettings {
    pub daemon: DaemonSettings,
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
        self.daemon.validate().context("invalid daemon settings")
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
    fn default_daemon_settings_match_current_behavior() {
        let settings = ShargonSettings::default();

        assert_eq!(
            settings.daemon.socket_path,
            PathBuf::from(DEFAULT_SOCKET_PATH)
        );
        assert_eq!(settings.daemon.readiness_timeout, Duration::from_secs(3));
        assert_eq!(settings.daemon.retry_interval, Duration::from_millis(50));
    }

    #[test]
    fn parses_valid_toml() -> anyhow::Result<()> {
        let temp_dir = tempdir()?;
        let config_path = temp_dir.path().join("shargon.toml");

        fs::write(
            &config_path,
            r#"
[daemon]
socket_path = "/tmp/custom-shargon.sock"
readiness_timeout = "5s"
retry_interval = "125ms"
"#,
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
        }
        .validate()
        .unwrap_err();

        assert!(format!("{err:#}").contains("readiness_timeout"));

        let err = ShargonSettings {
            daemon: DaemonSettings {
                retry_interval: Duration::ZERO,
                ..DaemonSettings::default()
            },
        }
        .validate()
        .unwrap_err();

        assert!(format!("{err:#}").contains("retry_interval"));
    }

    #[test]
    fn unknown_fields_are_rejected() -> anyhow::Result<()> {
        let temp_dir = tempdir()?;
        let config_path = temp_dir.path().join("shargon.toml");

        fs::write(
            &config_path,
            r#"
[daemon]
socket_path = "/tmp/custom-shargon.sock"
readiness_timeout = "5s"
retry_interval = "125ms"
unknown = "value"
"#,
        )?;

        let err = ShargonSettings::load_from_path(&config_path).unwrap_err();
        assert!(format!("{err:#}").contains("unknown field"));
        Ok(())
    }
}
