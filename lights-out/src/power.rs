use crate::profile::OperatingMode;
use anyhow::{ensure, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

struct CpuPolicy {
    path: PathBuf,
    min: u32,
    balanced_min: u32,
}

pub struct CpuPower {
    policies: Vec<CpuPolicy>,
}

fn read_frequency(path: &Path) -> Result<u32> {
    fs::read_to_string(path)
        .with_context(|| format!("Reading {}", path.display()))?
        .trim()
        .parse()
        .with_context(|| format!("Invalid frequency in {}", path.display()))
}

impl CpuPower {
    pub fn discover() -> Result<Self> {
        Self::from_path(Path::new("/sys/devices/system/cpu/cpufreq"))
    }

    fn from_path(root: &Path) -> Result<Self> {
        let mut policies = Vec::new();
        for entry in
            fs::read_dir(root).context("CPU frequency controls are required for Quiet mode")?
        {
            let entry = entry?;
            if !entry.file_name().to_string_lossy().starts_with("policy") {
                continue;
            }
            let path = entry.path();
            let min = read_frequency(&path.join("cpuinfo_min_freq"))?;
            let max = read_frequency(&path.join("cpuinfo_max_freq"))?;
            ensure!(min > 0 && max >= min, "Invalid CPU frequency range");
            let balanced_min = read_frequency(&path.join("amd_pstate_lowest_nonlinear_freq"))
                .unwrap_or(min)
                .clamp(min, max);
            policies.push(CpuPolicy {
                path,
                min,
                balanced_min,
            });
        }
        ensure!(
            !policies.is_empty(),
            "No CPU frequency policies found; cannot safely enable Quiet mode"
        );
        Ok(Self { policies })
    }

    pub fn ensure_profile(mode: OperatingMode) -> Result<()> {
        // Bound subprocess time so a stuck D-Bus daemon cannot stop temperature monitoring.
        let current = Command::new("timeout")
            .args(["2", "powerprofilesctl", "get"])
            .output()
            .context("Reading the power profile")?;
        ensure!(current.status.success(), "Cannot read the power profile");
        if String::from_utf8_lossy(&current.stdout).trim() != mode.power_profile() {
            let result = Command::new("timeout")
                .args(["2", "powerprofilesctl", "set", mode.power_profile()])
                .output()
                .context("Setting the power profile")?;
            ensure!(
                result.status.success(),
                "Cannot set power profile: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
        Ok(())
    }

    pub fn apply_limit(&self, mode: OperatingMode, quiet_max: u32) -> Result<()> {
        for policy in &self.policies {
            // power-profiles-daemon changes per-policy boost availability, so
            // cpuinfo_max_freq itself changes between Power Saver and Balanced.
            // Re-read it after applying the profile; caching it would silently
            // leave Balanced stuck at the Quiet profile's unboosted ceiling.
            let hardware_max = read_frequency(&policy.path.join("cpuinfo_max_freq"))?;
            ensure!(hardware_max >= policy.min, "Invalid CPU frequency range");
            let maximum = match mode {
                OperatingMode::Quiet => quiet_max.clamp(policy.min, hardware_max),
                OperatingMode::Balanced => hardware_max,
            };
            let minimum = if mode == OperatingMode::Quiet {
                policy.min
            } else {
                policy.balanced_min.min(hardware_max)
            };
            if read_frequency(&policy.path.join("scaling_min_freq"))? == minimum
                && read_frequency(&policy.path.join("scaling_max_freq"))? == maximum
            {
                continue;
            }
            // Lower the minimum before lowering the maximum; raise the maximum
            // before restoring the Balanced minimum. This avoids min > max.
            fs::write(policy.path.join("scaling_min_freq"), policy.min.to_string())?;
            fs::write(policy.path.join("scaling_max_freq"), maximum.to_string())?;
            if mode == OperatingMode::Balanced {
                fs::write(policy.path.join("scaling_min_freq"), minimum.to_string())?;
            }
            let actual = read_frequency(&policy.path.join("scaling_max_freq"))?;
            ensure!(
                actual == maximum,
                "CPU frequency limit did not take effect: requested {maximum}, got {actual}"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_limits_every_cpu_and_balanced_restores_hardware_limits() {
        let root = std::env::temp_dir().join(format!("lights-out-cpu-{}", std::process::id()));
        for index in [0, 1] {
            let p = root.join(format!("policy{index}"));
            fs::create_dir_all(&p).unwrap();
            for (name, value) in [
                ("cpuinfo_min_freq", 400_000),
                ("cpuinfo_max_freq", 4_400_000),
                ("amd_pstate_lowest_nonlinear_freq", 3_000_000),
                ("scaling_min_freq", 3_000_000),
                ("scaling_max_freq", 4_400_000),
            ] {
                fs::write(p.join(name), value.to_string()).unwrap();
            }
        }
        let cpu = CpuPower::from_path(&root).unwrap();
        cpu.apply_limit(OperatingMode::Quiet, 800_000).unwrap();
        for p in &cpu.policies {
            assert_eq!(
                read_frequency(&p.path.join("scaling_min_freq")).unwrap(),
                400_000
            );
            assert_eq!(
                read_frequency(&p.path.join("scaling_max_freq")).unwrap(),
                800_000
            );
        }
        // Simulate power-profiles-daemon re-enabling boost after discovery.
        for p in &cpu.policies {
            fs::write(p.path.join("cpuinfo_max_freq"), "5600000").unwrap();
        }
        cpu.apply_limit(OperatingMode::Balanced, 800_000).unwrap();
        for p in &cpu.policies {
            assert_eq!(
                read_frequency(&p.path.join("scaling_min_freq")).unwrap(),
                3_000_000
            );
            assert_eq!(
                read_frequency(&p.path.join("scaling_max_freq")).unwrap(),
                5_600_000
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}
