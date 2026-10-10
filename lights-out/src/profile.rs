use anyhow::{Context, Result};
use clap::ValueEnum;
use std::{fs, io, path::Path};

pub const MODE_PATH: &str = "/run/lights-out/mode";
pub const STATE_PATH: &str = "/run/lights-out/state";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum OperatingMode {
    #[default]
    Quiet,
    Balanced,
}

impl OperatingMode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Quiet => "quiet",
            Self::Balanced => "balanced",
        }
    }

    pub fn power_profile(self) -> &'static str {
        match self {
            Self::Quiet => "power-saver",
            Self::Balanced => "balanced",
        }
    }
}

pub fn read_mode(path: &Path) -> Result<OperatingMode> {
    match fs::read_to_string(path) {
        Ok(value) => OperatingMode::from_str(value.trim(), false).map_err(anyhow::Error::msg),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(OperatingMode::Quiet),
        Err(error) => Err(error).context("Reading cooling mode"),
    }
}

pub fn atomic_write(path: &Path, value: &str) -> Result<()> {
    let parent = path.parent().context("Missing state directory")?;
    fs::create_dir_all(parent).context("Creating cooling state directory (requires root)")?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temporary, value).context("Writing cooling state (requires root)")?;
    fs::rename(&temporary, path).context("Committing cooling state")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_selection_defaults_to_quiet_but_invalid_selection_fails() {
        let dir = std::env::temp_dir().join(format!("lights-out-mode-{}", std::process::id()));
        let path = dir.join("mode");
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(read_mode(&path).unwrap(), OperatingMode::Quiet);
        atomic_write(&path, "balanced\n").unwrap();
        assert_eq!(read_mode(&path).unwrap(), OperatingMode::Balanced);
        atomic_write(&path, "garbage\n").unwrap();
        assert!(read_mode(&path).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
