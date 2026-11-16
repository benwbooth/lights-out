//! Keeps the Corsair RAM dark while the daemon runs.
//!
//! The boot service sets both sticks to black, but they have been seen lit
//! again later in the same boot with nothing in the logs to explain it (seen
//! 2026-11-16). Re-sending black periodically through the OpenRGB server puts
//! them back out within a minute. Sending black to an already-dark stick is
//! invisible.

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const RAM_NAME: &str = "Corsair Dominator Platinum RGB DDR5";
const INTERVAL: Duration = Duration::from_secs(60);

/// Device indices the OpenRGB server lists for the RAM sticks.
fn ram_indices() -> Result<Vec<String>, String> {
    let output = Command::new("timeout")
        .args(["15", "openrgb", "--list-devices"])
        .output()
        .map_err(|error| format!("cannot run openrgb: {error}"))?;
    if !output.status.success() {
        return Err(format!("openrgb --list-devices failed ({})", output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once(": "))
        .filter(|(index, name)| name.trim() == RAM_NAME && index.parse::<u32>().is_ok())
        .map(|(index, _)| index.to_owned())
        .collect())
}

fn blacken() -> Result<usize, String> {
    let indices = ram_indices()?;
    if indices.is_empty() {
        return Err("no Corsair RAM listed by OpenRGB".to_owned());
    }
    for index in &indices {
        let status = Command::new("timeout")
            .args(["15", "openrgb", "--device", index, "--mode", "Direct", "--color", "000000"])
            .output()
            .map_err(|error| format!("cannot run openrgb: {error}"))?
            .status;
        if !status.success() {
            return Err(format!("setting RAM device {index} to black failed ({status})"));
        }
    }
    Ok(indices.len())
}

/// Runs until `stop_flag` is set. Failures are logged and retried next round;
/// they never affect cooling, which runs on another thread.
pub fn keep_dark(stop_flag: Arc<AtomicBool>) {
    let mut last_error = None;
    let mut logged_ok = false;
    // The boot service has just done this; start one interval later.
    let mut last_run = Instant::now();
    while !stop_flag.load(Ordering::Relaxed) {
        if last_run.elapsed() >= INTERVAL {
            match blacken() {
                Ok(count) => {
                    if !logged_ok || last_error.is_some() {
                        println!("  RAM lighting: keeping {count} stick(s) black every {}s", INTERVAL.as_secs());
                        logged_ok = true;
                    }
                    last_error = None;
                }
                Err(error) => {
                    if last_error.as_ref() != Some(&error) {
                        eprintln!("  RAM lighting not reapplied: {error}");
                    }
                    last_error = Some(error);
                }
            }
            last_run = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}
