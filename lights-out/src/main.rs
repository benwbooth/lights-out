use anyhow::{ensure, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use hidapi::{HidApi, HidDevice};
use i2cdev::core::I2CDevice;
use i2cdev::linux::LinuxI2CDevice;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod cooling;
use cooling::CoolingPolicy;

// MSI MPG CORELIQUID
mod msi {
    pub const VID: u16 = 0x0db0;
    pub const PID: u16 = 0xb130;
    pub const FEATURE_REPORT_ID: u8 = 0x52;
    pub const MAX_DATA_LEN: usize = 185;
    pub const HID_REPORT_LEN: usize = 64; // Includes the 0xD0 report ID
    pub const CMD_PREFIX: u8 = 0xD0;
    pub const CMD_LCD_DISABLE: u8 = 0x7F;
    pub const LED_MODE_DISABLE: u8 = 0;

    // Fan mode commands
    pub const CMD_FAN_MODE_1: u8 = 0x40;
    pub const CMD_FAN_MODE_2: u8 = 0x41;

    // CPU status command (for temperature reporting)
    pub const CMD_CPU_STATUS: u8 = 0x85;

    // Fan mode offsets in the command buffer (after cmd prefix and command byte)
    pub const FAN_MODE_OFFSETS: &[usize] = &[2, 10, 18, 26, 34];

    // Daemon polling interval in seconds
    pub const DAEMON_INTERVAL_SECS: u64 = 1;

    pub const LED_OFFSETS: &[usize] = &[
        1, 11, 21, 31, 42, 53, 74, 84, 94, 104, 114, 124, 134, 144, 154, 164, 174,
    ];
}

/// Fan modes for MSI CORELIQUID AIO cooler
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FanMode {
    /// Silent mode - quietest, lower cooling
    Silent = 0,
    /// Balance mode - balanced noise/cooling
    Balance = 1,
    /// Game mode - higher cooling, more noise
    Game = 2,
    /// Fixed 100% duty on all cooler fans and the pump
    Full = 3,
    /// Default mode - constant speed
    Default = 4,
    /// Smart mode - adapts to CPU temperature
    Smart = 5,
}

// LianLi UNI FAN AL V2 (from OpenRGB LianLiUniHubALController)
mod lianli {
    pub const VID: u16 = 0x0cf2;
    pub const PID: u16 = 0xa104;
    pub const TRANSACTION_ID: u8 = 0xe0;
    pub const PACKET_SIZE: usize = 65; // Standard packet size
    pub const COLOR_PACKET_SIZE: usize = 146; // Color data packet

    // Commit action command format: transaction_id, 0x10 + fan_or_edge + (channel*2), mode, speed, direction, brightness
    pub const MODE_STATIC: u8 = 0x01;
    pub const SPEED_VERY_SLOW: u8 = 0x02;
    pub const DIRECTION_LEFT_TO_RIGHT: u8 = 0x00;
    pub const BRIGHTNESS_OFF: u8 = 0x08; // 0% brightness

    pub const NUM_CHANNELS: u8 = 4;
}

// ASUS TUF Gaming GPU with ENE SMBus RGB controller
mod gpu {
    // ENE SMBus protocol (from OpenRGB ENESMBusController)
    pub const ENE_I2C_ADDR: u16 = 0x67;
    pub const ENE_REG_MODE: u16 = 0x8021;
    pub const ENE_REG_APPLY: u16 = 0x80A0;
    pub const ENE_MODE_OFF: u8 = 0x00;
    pub const ENE_APPLY_VAL: u8 = 0x01;

    // SMBus commands
    pub const SMBUS_CMD_ADDR: u8 = 0x00; // Register address selector (word)
    pub const SMBUS_CMD_DATA: u8 = 0x01; // Data write (byte)

    // Byte-swap for ENE protocol (little-endian on SMBus)
    pub fn swap_bytes(val: u16) -> u16 {
        ((val & 0xFF) << 8) | ((val >> 8) & 0xFF)
    }
}

#[derive(Parser)]
#[command(name = "ledctl")]
#[command(about = "Control RGB LEDs on various PC components")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Turn off all LEDs on all supported devices
    Off,
    /// Turn off MSI CORELIQUID cooler LEDs and LCD
    Msi,
    /// Turn off LianLi UNI FAN AL V2 LEDs
    Lianli,
    /// Turn off ASUS TUF Gaming GPU LEDs (via i2c)
    Gpu,
    /// Set MSI CORELIQUID cooler fan mode
    Fan {
        /// Fan mode to set
        #[arg(value_enum)]
        mode: FanMode,
    },
    /// Keep the cooler silent normally and increase cooling when the CPU is hot
    Daemon,
    /// Read cooler fan/pump RPM, duty and configured modes
    Status,
    /// Dump MSI cooler feature report (for debugging)
    Dump,
}

fn msi_disable() -> Result<()> {
    let api = HidApi::new().context("Failed to initialize HID API")?;
    let device = api
        .open(msi::VID, msi::PID)
        .context("Failed to open MSI CORELIQUID")?;

    // Disable LEDs via feature report
    let mut buf = [0u8; msi::MAX_DATA_LEN];
    buf[0] = msi::FEATURE_REPORT_ID;
    device
        .get_feature_report(&mut buf)
        .context("Failed to get feature report")?;

    for &offset in msi::LED_OFFSETS {
        if offset < msi::MAX_DATA_LEN {
            buf[offset] = msi::LED_MODE_DISABLE;
        }
    }
    device
        .send_feature_report(&buf)
        .context("Failed to send feature report")?;
    println!("  MSI CORELIQUID: LEDs disabled");

    // Disable LCD
    let mut cmd = [0u8; msi::HID_REPORT_LEN];
    cmd[0] = msi::CMD_PREFIX;
    cmd[1] = msi::CMD_LCD_DISABLE;
    device.write(&cmd).context("Failed to disable LCD")?;
    println!("  MSI CORELIQUID: LCD disabled");

    Ok(())
}

fn msi_set_fan_mode(mode: FanMode) -> Result<()> {
    let api = HidApi::new().context("Failed to initialize HID API")?;
    let device = api
        .open(msi::VID, msi::PID)
        .context("Failed to open MSI CORELIQUID")?;

    set_fan_mode(&device, mode)
}

// Protocol reference: liquidctl/docs/developer/protocol/coreliquid.md.
// Mode 3 is custom: Full supplies seven 100% duty points and zero temperatures.
fn fan_mode_reports(mode: FanMode) -> [[u8; msi::HID_REPORT_LEN]; 2] {
    let mut reports = [[0u8; msi::HID_REPORT_LEN]; 2];
    for (report, command) in reports
        .iter_mut()
        .zip([msi::CMD_FAN_MODE_1, msi::CMD_FAN_MODE_2])
    {
        report[0] = msi::CMD_PREFIX;
        report[1] = command;
        for &offset in msi::FAN_MODE_OFFSETS {
            report[offset] = mode as u8;
            if mode == FanMode::Full && command == msi::CMD_FAN_MODE_1 {
                report[offset + 1..offset + 8].fill(100);
            }
        }
    }
    reports
}

fn write_report(device: &HidDevice, report: &[u8]) -> Result<()> {
    let written = device
        .write(report)
        .context("Failed to write cooler command")?;
    ensure!(
        written == report.len(),
        "Short cooler command write: {written}/{}",
        report.len()
    );
    Ok(())
}

fn set_fan_mode(device: &HidDevice, mode: FanMode) -> Result<()> {
    for report in fan_mode_reports(mode) {
        write_report(device, &report)?;
    }
    println!("  MSI CORELIQUID: Fan mode set to {:?}", mode);
    Ok(())
}

/// Find the CPU temperature sensor in /sys/class/hwmon
/// Looks for k10temp (AMD) or coretemp (Intel) chips
fn find_cpu_temp_path() -> Result<std::path::PathBuf> {
    let hwmon_path = Path::new("/sys/class/hwmon");

    for entry in fs::read_dir(hwmon_path).context("Failed to read /sys/class/hwmon")? {
        let entry = entry?;
        let name_path = entry.path().join("name");

        if let Ok(name) = fs::read_to_string(&name_path) {
            let name = name.trim();
            // AMD CPUs use k10temp, Intel uses coretemp
            if name == "k10temp" || name == "coretemp" {
                // For k10temp, Tctl is usually temp1_input
                // For coretemp, package temp is also temp1_input
                let temp_path = entry.path().join("temp1_input");
                if temp_path.exists() {
                    return Ok(temp_path);
                }
            }
        }
    }

    anyhow::bail!("CPU temperature sensor not found (looking for k10temp or coretemp)")
}

/// Read CPU temperature in millidegrees Celsius, without rounding thresholds.
fn read_cpu_temp(temp_path: &Path) -> Result<i32> {
    let content = fs::read_to_string(temp_path).context("Failed to read temperature")?;
    let millidegrees: i32 = content
        .trim()
        .parse()
        .context("Failed to parse temperature")?;
    ensure!(
        (0..=150_000).contains(&millidegrees),
        "Invalid CPU temperature: {millidegrees}"
    );
    Ok(millidegrees)
}

/// Send CPU temperature to the AIO
fn send_cpu_temp(device: &HidDevice, temp: i32) -> Result<()> {
    let mut buf = [0u8; msi::HID_REPORT_LEN];
    buf[0] = msi::CMD_PREFIX;
    buf[1] = msi::CMD_CPU_STATUS;

    // Dummy CPU frequency (the AIO doesn't actually use this)
    let freq: u16 = 3000;
    buf[2] = (freq & 0xFF) as u8;
    buf[3] = ((freq >> 8) & 0xFF) as u8;

    // CPU temperature (little-endian)
    buf[4] = (temp & 0xFF) as u8;
    buf[5] = ((temp >> 8) & 0xFF) as u8;

    write_report(device, &buf).context("Failed to send CPU temperature")?;
    Ok(())
}

/// Run the temperature monitoring daemon
fn msi_daemon(stop_flag: Arc<AtomicBool>) -> Result<()> {
    let api = HidApi::new().context("Failed to initialize HID API")?;
    let device = api
        .open(msi::VID, msi::PID)
        .context("Failed to open MSI CORELIQUID")?;

    // Never leave a cooler at a quiet setting if the sensor cannot be found.
    let temp_path = match find_cpu_temp_path() {
        Ok(path) => path,
        Err(error) => {
            set_fan_mode(&device, FanMode::Full)?;
            return Err(error);
        }
    };
    println!("  Found CPU temp sensor: {}", temp_path.display());
    println!("  Cooling policy: Silent normally; Game at 80°C; Full at 85°C; Silent again after 30 seconds below 70°C");

    let started = Instant::now();
    let mut policy = CoolingPolicy::default();
    let mut applied_mode = None;
    let mut last_applied = Instant::now();
    let mut last_logged = Instant::now();

    while !stop_flag.load(Ordering::Relaxed) {
        let temperature = read_cpu_temp(&temp_path);
        let mode = policy.update(temperature.as_ref().ok().copied(), started.elapsed());
        let changed = applied_mode != Some(mode);
        // Periodically reassert the policy after another program changes modes.
        if changed || last_applied.elapsed() >= Duration::from_secs(10) {
            if let Err(error) = set_fan_mode(&device, mode) {
                let _ = set_fan_mode(&device, FanMode::Full);
                return Err(error);
            }
            applied_mode = Some(mode);
            last_applied = Instant::now();
        }
        match temperature {
            Ok(millidegrees) => {
                if changed || last_logged.elapsed() >= Duration::from_secs(10) {
                    println!(
                        "  CPU Temperature: {:.1}°C; cooling: {mode:?}",
                        millidegrees as f64 / 1000.0
                    );
                    last_logged = Instant::now();
                }
                if let Err(error) = send_cpu_temp(&device, millidegrees / 1000) {
                    let _ = set_fan_mode(&device, FanMode::Full);
                    return Err(error);
                }
            }
            Err(error) => {
                // Systemd retries sensor discovery, while full speed remains set.
                return Err(error);
            }
        }

        // Sleep for the interval, checking stop flag periodically
        for _ in 0..(msi::DAEMON_INTERVAL_SECS * 10) {
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    // A stopped daemon cannot keep providing fresh temperatures. Use a fixed
    // duty that does not depend on the last (possibly cool) temperature sample.
    set_fan_mode(&device, FanMode::Full)?;
    println!("  Daemon stopped; cooler left at full speed.");
    Ok(())
}

fn read_cooler_report(device: &HidDevice, command: u8) -> Result<Vec<u8>> {
    // Discard old replies, with a bound in case a device continuously reports.
    let mut response = [0u8; msi::HID_REPORT_LEN];
    for _ in 0..32 {
        if device.read_timeout(&mut response, 0)? == 0 {
            break;
        }
    }
    let mut request = [0u8; msi::HID_REPORT_LEN];
    request[0] = msi::CMD_PREFIX;
    request[1] = command;
    write_report(device, &request)?;
    // Both configuration queries reply with 0x32, including the 0x33
    // temperature query (confirmed by the MSI protocol and device readback).
    let reply_command = if command == 0x33 { 0x32 } else { command };
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let length = device.read_timeout(&mut response, 200)?;
        if length >= 2 && response[0] == msi::CMD_PREFIX && response[1] == reply_command {
            return Ok(response[..length].to_vec());
        }
    }
    anyhow::bail!("Timed out reading cooler report 0x{command:02x}")
}

fn msi_status() -> Result<()> {
    let api = HidApi::new().context("Failed to initialize HID API")?;
    let device = api
        .open(msi::VID, msi::PID)
        .context("Failed to open MSI CORELIQUID")?;
    let status = read_cooler_report(&device, 0x31)?;
    let speeds = read_cooler_report(&device, 0x32)?;
    let temperatures = read_cooler_report(&device, 0x33)?;
    ensure!(
        status.len() >= 32 && speeds.len() >= 42 && temperatures.len() >= 42,
        "Short cooler status reply"
    );
    // Channel order follows the liquidctl MSI driver (the protocol document's
    // pump/waterblock labels are reversed).
    for (index, name) in [
        "Radiator fan 1",
        "Radiator fan 2",
        "Radiator fan 3",
        "Waterblock fan",
        "Pump",
    ]
    .iter()
    .enumerate()
    {
        let rpm_offset = 2 + index * 2;
        let duty_offset = 22 + index * 2;
        let mode_offset = 2 + index * 8;
        let rpm = u16::from_le_bytes([status[rpm_offset], status[rpm_offset + 1]]);
        let duty = u16::from_le_bytes([status[duty_offset], status[duty_offset + 1]]);
        println!(
            "  {name}: {rpm} RPM, {duty}% duty, speed mode {}, temperature mode {}",
            speeds[mode_offset], temperatures[mode_offset]
        );
    }
    Ok(())
}

fn lianli_disable() -> Result<()> {
    let api = HidApi::new().context("Failed to initialize HID API")?;

    // Find the device by iterating (like uni-sync does)
    let device_info = api
        .device_list()
        .find(|d| d.vendor_id() == lianli::VID && d.product_id() == lianli::PID)
        .context("LianLi UNI FAN AL V2 not found")?;

    let device = api
        .open_path(device_info.path())
        .context("Failed to open LianLi UNI FAN AL V2")?;

    // Following OpenRGB LianLiUniHubALController protocol:
    // 1. Send color data (all black) - 146 byte packet
    // 2. Send commit action with 0% brightness - 65 byte packet

    for channel in 0..lianli::NUM_CHANNELS {
        // Send black color data for fan LEDs (register 0x30 + channel*2)
        let mut color_packet = [0u8; lianli::COLOR_PACKET_SIZE];
        color_packet[0] = lianli::TRANSACTION_ID;
        color_packet[1] = 0x30 + (channel * 2); // Fan LEDs
                                                // Rest is zeros (black RGB)
        match device.write(&color_packet) {
            Ok(_) => {}
            Err(e) => eprintln!("    Warning: color packet ch{} fan failed: {}", channel, e),
        }
        std::thread::sleep(std::time::Duration::from_millis(20));

        // Send black color data for edge LEDs (register 0x31 + channel*2)
        color_packet[1] = 0x31 + (channel * 2); // Edge LEDs
        match device.write(&color_packet) {
            Ok(_) => {}
            Err(e) => eprintln!("    Warning: color packet ch{} edge failed: {}", channel, e),
        }
        std::thread::sleep(std::time::Duration::from_millis(20));

        // Commit action for fan LEDs - 65 byte packet
        let mut commit = [0u8; lianli::PACKET_SIZE];
        commit[0] = lianli::TRANSACTION_ID;
        commit[1] = 0x10 + (channel * 2); // Fan LEDs commit register
        commit[2] = lianli::MODE_STATIC;
        commit[3] = lianli::SPEED_VERY_SLOW;
        commit[4] = lianli::DIRECTION_LEFT_TO_RIGHT;
        commit[5] = lianli::BRIGHTNESS_OFF;
        device
            .write(&commit)
            .context("Failed to write fan LED commit")?;
        std::thread::sleep(std::time::Duration::from_millis(20));

        // Commit action for edge LEDs
        commit[1] = 0x11 + (channel * 2); // Edge LEDs commit register
        device
            .write(&commit)
            .context("Failed to write edge LED commit")?;
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    println!("  LianLi UNI FAN AL V2: LEDs disabled (static black, 0% brightness)");
    Ok(())
}

/// Find the AMDGPU OEM i2c bus by scanning /sys/class/i2c-dev/*/name
fn find_gpu_i2c_bus() -> Result<String> {
    let i2c_dev_path = Path::new("/sys/class/i2c-dev");

    for entry in fs::read_dir(i2c_dev_path).context("Failed to read /sys/class/i2c-dev")? {
        let entry = entry?;
        let name_path = entry.path().join("name");
        if let Ok(name) = fs::read_to_string(&name_path) {
            // Look for "AMDGPU DM i2c OEM bus" or similar
            if name.contains("AMDGPU") && name.contains("OEM") {
                let dev_name = entry.file_name();
                let bus_path = format!("/dev/{}", dev_name.to_string_lossy());
                return Ok(bus_path);
            }
        }
    }

    anyhow::bail!("AMDGPU OEM i2c bus not found. Ensure kernel >= 6.14 with OEM i2c patches.")
}

fn gpu_disable() -> Result<()> {
    let bus_path = find_gpu_i2c_bus()?;
    println!("  GPU: Found i2c bus at {}", bus_path);

    let mut device = LinuxI2CDevice::new(&bus_path, gpu::ENE_I2C_ADDR)
        .context("Failed to open GPU i2c device")?;

    // Set LED mode to OFF
    // Write register address (byte-swapped)
    device
        .smbus_write_word_data(gpu::SMBUS_CMD_ADDR, gpu::swap_bytes(gpu::ENE_REG_MODE))
        .context("Failed to write mode register address")?;
    // Write mode value
    device
        .smbus_write_byte_data(gpu::SMBUS_CMD_DATA, gpu::ENE_MODE_OFF)
        .context("Failed to write mode value")?;

    // Apply changes
    device
        .smbus_write_word_data(gpu::SMBUS_CMD_ADDR, gpu::swap_bytes(gpu::ENE_REG_APPLY))
        .context("Failed to write apply register address")?;
    device
        .smbus_write_byte_data(gpu::SMBUS_CMD_DATA, gpu::ENE_APPLY_VAL)
        .context("Failed to write apply value")?;

    println!("  GPU: LEDs disabled");
    Ok(())
}

fn msi_dump() -> Result<()> {
    let api = HidApi::new().context("Failed to initialize HID API")?;
    let device = api
        .open(msi::VID, msi::PID)
        .context("Failed to open MSI CORELIQUID")?;

    let mut buf = [0u8; msi::MAX_DATA_LEN];
    buf[0] = msi::FEATURE_REPORT_ID;
    device.get_feature_report(&mut buf)?;

    println!(
        "Feature report 0x{:02X} ({} bytes):",
        msi::FEATURE_REPORT_ID,
        msi::MAX_DATA_LEN
    );
    for (i, chunk) in buf.chunks(16).enumerate() {
        print!("{:04x}: ", i * 16);
        for b in chunk {
            print!("{:02x} ", b);
        }
        println!();
    }

    println!("\nLED area modes:");
    for &offset in msi::LED_OFFSETS {
        if offset < msi::MAX_DATA_LEN {
            println!("  Offset {:3}: mode = {}", offset, buf[offset]);
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Off => {
            println!("Disabling all RGB LEDs...\n");

            if let Err(e) = msi_disable() {
                println!("  MSI CORELIQUID: not found or error: {}", e);
            }

            if let Err(e) = lianli_disable() {
                println!("  LianLi UNI FAN: not found or error: {}", e);
            }

            if let Err(e) = gpu_disable() {
                println!("  GPU: not found or error: {}", e);
            }

            println!("\nDone!");
            Ok(())
        }
        Commands::Msi => {
            println!("Disabling MSI CORELIQUID LEDs...");
            msi_disable()
        }
        Commands::Lianli => {
            println!("Disabling LianLi UNI FAN AL V2 LEDs...");
            lianli_disable()
        }
        Commands::Gpu => {
            println!("Disabling GPU LEDs...");
            gpu_disable()
        }
        Commands::Fan { mode } => {
            println!("Setting MSI CORELIQUID fan mode...");
            msi_set_fan_mode(mode)
        }
        Commands::Daemon => {
            println!("Starting MSI CORELIQUID temperature daemon...");

            // Set up signal handler for graceful shutdown
            let stop_flag = Arc::new(AtomicBool::new(false));
            let stop_flag_clone = stop_flag.clone();

            ctrlc::set_handler(move || {
                println!("\n  Received shutdown signal...");
                stop_flag_clone.store(true, Ordering::Relaxed);
            })
            .context("Failed to set signal handler")?;

            msi_daemon(stop_flag)
        }
        Commands::Dump => msi_dump(),
        Commands::Status => msi_status(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emergency_mode_sets_every_duty_point_to_full() {
        let [speeds, temperatures] = fan_mode_reports(FanMode::Full);
        assert_eq!(&speeds[..2], &[0xd0, 0x40]);
        assert_eq!(&temperatures[..2], &[0xd0, 0x41]);
        for offset in [2, 10, 18, 26, 34] {
            assert_eq!(
                &speeds[offset..offset + 8],
                &[3, 100, 100, 100, 100, 100, 100, 100]
            );
            assert_eq!(&temperatures[offset..offset + 8], &[3, 0, 0, 0, 0, 0, 0, 0]);
        }
    }

    #[test]
    fn preset_modes_keep_the_firmware_curve() {
        for mode in [FanMode::Silent, FanMode::Game] {
            for report in fan_mode_reports(mode) {
                for offset in [2, 10, 18, 26, 34] {
                    assert_eq!(report[offset], mode as u8);
                    assert_eq!(&report[offset + 1..offset + 8], &[0; 7]);
                }
            }
        }
    }
}
