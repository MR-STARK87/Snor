//! Physical display brightness, the platform layer behind Dim Mode.
//!
//! Windows laptops expose the backlight through WMI (`WmiMonitorBrightness`
//! for reading, `WmiMonitorBrightnessMethods.WmiSetBrightness` for writing
//! under `root/WMI`). This module reaches those two calls through PowerShell
//! so Dim Mode needs no new crates: the app already shells out to
//! `powershell.exe` for its terminal. Desktops and external monitors usually
//! expose nothing there, which surfaces as an `Err` the caller shows as a
//! small status message without interrupting anything else.
//!
//! No polling lives here: the backend only runs when Dim Mode toggles or
//! restores, so there is no background cost while agents run.

/// Brightness applied while Dim Mode is active (percent).
pub const DEFAULT_DIM_LEVEL: u8 = 10;
/// WMI accepts 0-100; 0 would black the screen out, so the floor is 1.
const MIN_LEVEL: u8 = 1;
const MAX_LEVEL: u8 = 100;

/// Clamp a requested level into the range the backend accepts.
pub fn clamp_level(level: u8) -> u8 {
    level.clamp(MIN_LEVEL, MAX_LEVEL)
}

/// Parse the output of the brightness query: the first 0-100 value on any
/// line. Multiple monitors report one value each; the first is enough to
/// restore the user's level on the way back out.
pub fn parse_brightness_output(out: &str) -> Result<u8, String> {
    out.split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<u32>().ok())
        .find(|&v| v <= u32::from(MAX_LEVEL))
        .map(|v| v as u8)
        .ok_or_else(|| "no brightness value reported (external display?)".to_string())
}

/// PowerShell that prints the current brightness (one value per monitor).
#[cfg(windows)]
fn query_script() -> &'static str {
    "(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness | Select-Object -ExpandProperty CurrentBrightness)"
}

/// PowerShell that sets every monitor to `level`.
///
/// `Timeout = 0` keeps the level until it is changed again. A nonzero
/// timeout would let Windows revert the brightness on its own, which would
/// silently undo Dim Mode (and its saved restore value) behind our back.
#[cfg(windows)]
fn set_script(level: u8) -> String {
    format!(
        "$ms = Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods; \
         if (-not $ms) {{ Write-Error 'no brightness control on this hardware'; exit 42 }}; \
         $ms | Invoke-CimMethod -MethodName WmiSetBrightness -Arguments @{{ Brightness = {level}; Timeout = 0 }} | Out-Null"
    )
}

#[cfg(windows)]
fn run_powershell(script: &str) -> Result<std::process::Output, String> {
    use std::os::windows::process::CommandExt;
    // Do not flash a console window for a background brightness call.
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("could not query display brightness: {e}"))
}

/// Current display brightness, 0-100.
#[cfg(windows)]
pub fn get_brightness() -> Result<u8, String> {
    let out = run_powershell(query_script())?;
    if !out.status.success() {
        let detail = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if detail.is_empty() {
            "display brightness is unavailable on this hardware".to_string()
        } else {
            format!("display brightness is unavailable: {detail}")
        });
    }
    parse_brightness_output(&String::from_utf8_lossy(&out.stdout))
}

/// Set the display brightness to `level` (clamped to 1-100).
#[cfg(windows)]
pub fn set_brightness(level: u8) -> Result<(), String> {
    let level = clamp_level(level);
    let out = run_powershell(&set_script(level))?;
    if !out.status.success() {
        let detail = String::from_utf8_lossy(&out.stderr).trim().to_string();
        // Cap the detail: WMI errors can ramble, and this ends up in the
        // status bar.
        let detail: String = detail.chars().take(160).collect();
        return Err(if detail.is_empty() {
            "could not set display brightness on this hardware".to_string()
        } else {
            format!("could not set display brightness: {detail}")
        });
    }
    Ok(())
}

/// Brightness control is a Windows-first feature; anywhere else the backend
/// reports unavailable so the UI can say so instead of crashing.
#[cfg(not(windows))]
pub fn get_brightness() -> Result<u8, String> {
    Err("display brightness control is only supported on Windows".to_string())
}

/// See [`get_brightness`].
#[cfg(not(windows))]
pub fn set_brightness(_level: u8) -> Result<(), String> {
    Err("display brightness control is only supported on Windows".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_single_reported_value() {
        assert_eq!(parse_brightness_output("80\r\n"), Ok(80));
        assert_eq!(parse_brightness_output("  100  "), Ok(100));
    }

    #[test]
    fn takes_the_first_value_when_several_monitors_report() {
        assert_eq!(parse_brightness_output("80\r\n75\r\n"), Ok(80));
    }

    #[test]
    fn reports_unavailable_when_nothing_parseable_comes_back() {
        assert!(parse_brightness_output("").is_err());
        assert!(parse_brightness_output("no instances").is_err());
    }

    #[test]
    fn levels_stay_inside_what_wmi_accepts() {
        assert_eq!(clamp_level(0), 1);
        assert_eq!(clamp_level(10), 10);
        assert_eq!(clamp_level(200), 100);
    }

    #[test]
    fn default_dim_level_is_ten_percent() {
        assert_eq!(DEFAULT_DIM_LEVEL, 10);
    }

    #[cfg(windows)]
    #[test]
    fn set_script_carries_the_level_and_a_sticky_timeout() {
        let s = set_script(10);
        assert!(s.contains("Brightness = 10"), "level missing: {s}");
        assert!(
            s.contains("Timeout = 0"),
            "a nonzero timeout would let Windows revert Dim Mode: {s}"
        );
    }
}
