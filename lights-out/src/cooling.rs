use crate::FanMode;
use std::time::Duration;

pub const QUIET_MAX_KHZ: u32 = 3_600_000;
pub const QUIET_MIN_KHZ: u32 = 800_000;

/// Quiet sheds CPU performance before changing fan speed. The emergency
/// override is reserved for an unsafe temperature or a failed sensor.
pub struct QuietPolicy {
    limit: u32,
    emergency: bool,
    recovered_since: Option<Duration>,
    cool_since: Option<Duration>,
}

impl Default for QuietPolicy {
    fn default() -> Self {
        Self {
            limit: QUIET_MAX_KHZ,
            emergency: false,
            recovered_since: None,
            cool_since: None,
        }
    }
}

impl QuietPolicy {
    pub fn update(&mut self, temperature: Option<i32>, now: Duration) -> (FanMode, u32) {
        let Some(temperature) = temperature.filter(|value| (0..85_000).contains(value)) else {
            self.limit = QUIET_MIN_KHZ;
            self.emergency = true;
            self.recovered_since = None;
            self.cool_since = None;
            return (FanMode::Full, self.limit);
        };
        if self.emergency {
            if temperature < 70_000 {
                let since = self.recovered_since.get_or_insert(now);
                if now.saturating_sub(*since) >= Duration::from_secs(10) {
                    self.emergency = false;
                    self.recovered_since = None;
                }
            } else {
                self.recovered_since = None;
            }
        }

        if temperature >= 80_000 {
            self.limit = QUIET_MIN_KHZ;
        } else if temperature >= 75_000 {
            self.limit = self.limit.min(1_200_000);
        } else if temperature >= 68_000 {
            self.limit = self.limit.saturating_sub(400_000).max(QUIET_MIN_KHZ);
        }
        if temperature < 60_000 && !self.emergency {
            let since = self.cool_since.get_or_insert(now);
            if now.saturating_sub(*since) >= Duration::from_secs(10) {
                self.limit = (self.limit + 200_000).min(QUIET_MAX_KHZ);
                self.cool_since = Some(now);
            }
        } else {
            self.cool_since = None;
        }
        (
            if self.emergency {
                FanMode::Full
            } else {
                FanMode::Quiet
            },
            self.limit,
        )
    }
}

/// Escalate immediately; only reduce cooling after a sustained cool interval.
/// Temperature thresholds are in millidegrees Celsius and elapsed time is
/// monotonic. Invalid/missing samples always request fixed full speed.
pub struct CoolingPolicy {
    mode: FanMode,
    cool_since: Option<Duration>,
}

impl Default for CoolingPolicy {
    fn default() -> Self {
        Self {
            mode: FanMode::Silent,
            cool_since: None,
        }
    }
}

impl CoolingPolicy {
    pub fn update(&mut self, temperature: Option<i32>, now: Duration) -> FanMode {
        let Some(temperature) = temperature.filter(|value| (0..85_000).contains(value)) else {
            self.mode = FanMode::Full;
            self.cool_since = None;
            return self.mode;
        };

        if temperature >= 80_000 && self.mode == FanMode::Silent {
            self.mode = FanMode::Game;
        }

        // Release the emergency override once there is thermal headroom. Using
        // the quiet-mode threshold for Full as well used to latch all channels
        // at 100% through ordinary workloads in the 70-79 degree range.
        let cooldown = match self.mode {
            FanMode::Full if temperature < 80_000 => Some((10, FanMode::Game)),
            FanMode::Game if temperature < 75_000 => Some((15, FanMode::Silent)),
            _ => None,
        };
        if let Some((seconds, next_mode)) = cooldown {
            let since = self.cool_since.get_or_insert(now);
            if now.saturating_sub(*since) >= Duration::from_secs(seconds) {
                self.mode = next_mode;
                self.cool_since = None;
            }
        } else {
            self.cool_since = None;
        }
        self.mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_warm_load_reduces_cpu_instead_of_raising_fans() {
        let mut policy = QuietPolicy::default();
        assert_eq!(
            policy.update(Some(55_000), Duration::ZERO),
            (FanMode::Quiet, 3_600_000)
        );
        assert_eq!(
            policy.update(Some(68_000), Duration::from_secs(1)),
            (FanMode::Quiet, 3_200_000)
        );
        assert_eq!(
            policy.update(Some(70_000), Duration::from_secs(2)),
            (FanMode::Quiet, 2_800_000)
        );
        assert_eq!(
            policy.update(Some(75_000), Duration::from_secs(3)),
            (FanMode::Quiet, 1_200_000)
        );
        assert_eq!(
            policy.update(Some(84_999), Duration::from_secs(4)),
            (FanMode::Quiet, 800_000)
        );
        assert_eq!(
            policy.update(Some(59_000), Duration::from_secs(5)),
            (FanMode::Quiet, 800_000)
        );
        assert_eq!(
            policy.update(Some(59_000), Duration::from_secs(14)),
            (FanMode::Quiet, 800_000)
        );
        assert_eq!(
            policy.update(Some(59_000), Duration::from_secs(15)),
            (FanMode::Quiet, 1_000_000)
        );
        assert_eq!(
            policy.update(Some(62_000), Duration::from_secs(16)),
            (FanMode::Quiet, 1_000_000)
        );
        assert_eq!(
            policy.update(Some(59_000), Duration::from_secs(26)),
            (FanMode::Quiet, 1_000_000)
        );
    }

    #[test]
    fn quiet_emergency_requires_sustained_recovery_and_limits_cpu() {
        let mut policy = QuietPolicy::default();
        assert_eq!(
            policy.update(Some(85_000), Duration::ZERO),
            (FanMode::Full, 800_000)
        );
        assert_eq!(
            policy.update(Some(69_000), Duration::from_secs(1)),
            (FanMode::Full, 800_000)
        );
        assert_eq!(
            policy.update(Some(70_000), Duration::from_secs(11)),
            (FanMode::Full, 800_000)
        );
        assert_eq!(
            policy.update(Some(55_000), Duration::from_secs(12)),
            (FanMode::Full, 800_000)
        );
        assert_eq!(
            policy.update(Some(55_000), Duration::from_secs(22)),
            (FanMode::Quiet, 800_000)
        );
        assert_eq!(
            policy.update(None, Duration::from_secs(23)),
            (FanMode::Full, 800_000)
        );
        assert_eq!(
            policy.update(Some(-1), Duration::from_secs(24)),
            (FanMode::Full, 800_000)
        );
    }

    fn sample(policy: &mut CoolingPolicy, temperature: i32, second: u64) -> FanMode {
        policy.update(Some(temperature), Duration::from_secs(second))
    }

    #[test]
    fn quiet_workload_does_not_change_modes() {
        let mut policy = CoolingPolicy::default();
        for (second, temperature) in [40_000, 60_000, 79_999, 72_000, 50_000]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                sample(&mut policy, temperature, second as u64),
                FanMode::Silent
            );
        }
    }

    #[test]
    fn hot_start_and_spikes_escalate_without_delay() {
        let mut policy = CoolingPolicy::default();
        assert_eq!(sample(&mut policy, 80_000, 0), FanMode::Game);
        assert_eq!(sample(&mut policy, 84_999, 1), FanMode::Game);
        assert_eq!(sample(&mut policy, 85_000, 2), FanMode::Full);
        assert_eq!(
            sample(&mut CoolingPolicy::default(), 92_000, 0),
            FanMode::Full
        );
    }

    #[test]
    fn full_speed_steps_down_after_ten_seconds_continuously_below_eighty() {
        let mut policy = CoolingPolicy::default();
        assert_eq!(sample(&mut policy, 91_000, 0), FanMode::Full);
        assert_eq!(sample(&mut policy, 79_999, 1), FanMode::Full);
        assert_eq!(sample(&mut policy, 75_000, 10), FanMode::Full);
        assert_eq!(sample(&mut policy, 80_000, 11), FanMode::Full);
        assert_eq!(sample(&mut policy, 79_000, 12), FanMode::Full);
        assert_eq!(sample(&mut policy, 79_000, 21), FanMode::Full);
        assert_eq!(sample(&mut policy, 79_000, 22), FanMode::Game);
        assert_eq!(sample(&mut policy, 79_000, 100), FanMode::Game);
    }

    #[test]
    fn game_mode_returns_to_silent_after_cooldown() {
        let mut policy = CoolingPolicy::default();
        assert_eq!(sample(&mut policy, 80_000, 0), FanMode::Game);
        assert_eq!(sample(&mut policy, 74_999, 1), FanMode::Game);
        assert_eq!(sample(&mut policy, 74_000, 15), FanMode::Game);
        assert_eq!(sample(&mut policy, 75_000, 16), FanMode::Game);
        assert_eq!(sample(&mut policy, 74_000, 17), FanMode::Game);
        assert_eq!(sample(&mut policy, 74_000, 31), FanMode::Game);
        assert_eq!(sample(&mut policy, 74_000, 32), FanMode::Silent);
        assert_eq!(sample(&mut policy, 80_000, 33), FanMode::Game);
    }

    #[test]
    fn renewed_heat_during_cooldown_escalates_immediately() {
        let mut policy = CoolingPolicy::default();
        assert_eq!(sample(&mut policy, 85_000, 0), FanMode::Full);
        assert_eq!(sample(&mut policy, 60_000, 1), FanMode::Full);
        assert_eq!(sample(&mut policy, 60_000, 11), FanMode::Game);
        assert_eq!(sample(&mut policy, 60_000, 12), FanMode::Game);
        assert_eq!(sample(&mut policy, 85_000, 26), FanMode::Full);
        assert_eq!(sample(&mut policy, 60_000, 27), FanMode::Full);
        assert_eq!(sample(&mut policy, 60_000, 37), FanMode::Game);
        assert_eq!(sample(&mut policy, 60_000, 38), FanMode::Game);
        assert_eq!(sample(&mut policy, 60_000, 52), FanMode::Game);
        assert_eq!(sample(&mut policy, 60_000, 53), FanMode::Silent);
    }

    #[test]
    fn missing_sensor_forces_full_and_resets_cooldown() {
        let mut policy = CoolingPolicy::default();
        assert_eq!(policy.update(None, Duration::ZERO), FanMode::Full);
        assert_eq!(sample(&mut policy, 50_000, 1), FanMode::Full);
        assert_eq!(policy.update(None, Duration::from_secs(10)), FanMode::Full);
        assert_eq!(sample(&mut policy, 50_000, 11), FanMode::Full);
        assert_eq!(sample(&mut policy, 50_000, 20), FanMode::Full);
        assert_eq!(sample(&mut policy, 50_000, 21), FanMode::Game);
        assert_eq!(sample(&mut policy, -1, 22), FanMode::Full);
    }
}
