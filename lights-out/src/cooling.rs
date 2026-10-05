use crate::FanMode;
use std::time::Duration;

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
        match temperature {
            Some(temperature) if (0..85_000).contains(&temperature) => {
                if temperature >= 80_000 && self.mode == FanMode::Silent {
                    self.mode = FanMode::Game;
                }
                if temperature < 70_000 && self.mode != FanMode::Silent {
                    let since = self.cool_since.get_or_insert(now);
                    if now.saturating_sub(*since) >= Duration::from_secs(30) {
                        self.mode = FanMode::Silent;
                        self.cool_since = None;
                    }
                } else {
                    self.cool_since = None;
                }
            }
            _ => {
                self.mode = FanMode::Full;
                self.cool_since = None;
            }
        }
        self.mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn full_speed_stays_until_thirty_seconds_continuously_below_seventy() {
        let mut policy = CoolingPolicy::default();
        assert_eq!(sample(&mut policy, 91_000, 0), FanMode::Full);
        assert_eq!(sample(&mut policy, 75_000, 10), FanMode::Full);
        assert_eq!(sample(&mut policy, 69_000, 20), FanMode::Full);
        assert_eq!(sample(&mut policy, 68_000, 49), FanMode::Full);
        assert_eq!(sample(&mut policy, 70_000, 50), FanMode::Full);
        assert_eq!(sample(&mut policy, 68_000, 51), FanMode::Full);
        assert_eq!(sample(&mut policy, 65_000, 80), FanMode::Full);
        assert_eq!(sample(&mut policy, 65_000, 81), FanMode::Silent);
    }

    #[test]
    fn game_mode_returns_to_silent_after_cooldown() {
        let mut policy = CoolingPolicy::default();
        assert_eq!(sample(&mut policy, 80_000, 0), FanMode::Game);
        assert_eq!(sample(&mut policy, 69_999, 1), FanMode::Game);
        assert_eq!(sample(&mut policy, 65_000, 31), FanMode::Silent);
        assert_eq!(sample(&mut policy, 80_000, 32), FanMode::Game);
    }

    #[test]
    fn missing_sensor_forces_full_and_resets_cooldown() {
        let mut policy = CoolingPolicy::default();
        assert_eq!(policy.update(None, Duration::ZERO), FanMode::Full);
        assert_eq!(sample(&mut policy, 50_000, 1), FanMode::Full);
        assert_eq!(policy.update(None, Duration::from_secs(30)), FanMode::Full);
        assert_eq!(sample(&mut policy, 50_000, 31), FanMode::Full);
        assert_eq!(sample(&mut policy, 50_000, 60), FanMode::Full);
        assert_eq!(sample(&mut policy, 50_000, 61), FanMode::Silent);
        assert_eq!(sample(&mut policy, -1, 62), FanMode::Full);
    }
}
