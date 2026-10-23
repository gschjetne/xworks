//! What the shell reads of the system's own settings for the person's
//! defaults (review ydqhfds0): the caret's blink rate, and whether
//! measurements are in inches or centimetres.
//!
//! Each platform's settings are read where they're kept, without unsafe
//! code of our own (CLAUDE.md, rule 4): Windows' registry, through
//! windows-registry's safe API; GNOME's settings through `gsettings`,
//! KDE's in `kdeglobals`; macOS's user defaults through `defaults`. The
//! readers that spawn a program or read a file are kept out of the start
//! (see `Shell::open`'s thread); `app::BlinkRate` makes sense of what
//! they find.

use std::path::PathBuf;
use std::process::Command;

use app::{BlinkRate, Blinking};

/// How the system has the caret blink, or Windows' default, 530
/// milliseconds each way, where it can't be found. It may spawn a program
/// and wait for it, so it's called off the event loop.
pub(crate) fn blinking() -> Blinking {
    let found = if cfg!(target_os = "windows") {
        windows_blinking()
    } else if cfg!(target_os = "macos") {
        mac_blinking()
    } else {
        unix_blinking()
    };
    found.unwrap_or(Blinking::At(BlinkRate::DEFAULT))
}

/// Windows': `CursorBlinkRate` and `CaretTimeout` in
/// `HKCU\Control Panel\Desktop`, as the Accessibility settings set them.
#[cfg(target_os = "windows")]
fn windows_blinking() -> Option<Blinking> {
    let desktop = windows_registry::CURRENT_USER
        .open(r"Control Panel\Desktop")
        .ok()?;
    let rate = desktop.get_string("CursorBlinkRate").ok()?;
    let timeout = desktop.get_u32("CaretTimeout").ok();
    BlinkRate::windows(&rate, timeout)
}

#[cfg(not(target_os = "windows"))]
fn windows_blinking() -> Option<Blinking> {
    None
}

/// macOS's: the user defaults `NSTextInsertionPointBlinkPeriodOn` and
/// `...Off`, where they're set.
fn mac_blinking() -> Option<Blinking> {
    let read = |key: &str| {
        Command::new("defaults")
            .args(["read", "-g", key])
            .output()
            .ok()
            .filter(|out| out.status.success())
            .and_then(|out| {
                String::from_utf8_lossy(&out.stdout)
                    .trim()
                    .parse::<f64>()
                    .ok()
            })
    };
    BlinkRate::mac(
        read("NSTextInsertionPointBlinkPeriodOn"),
        read("NSTextInsertionPointBlinkPeriodOff"),
    )
}

/// Linux's and the BSDs': KDE's in `kdeglobals` under KDE, and GNOME's
/// through `gsettings` elsewhere, as GTK reads them on every desktop that
/// has them.
fn unix_blinking() -> Option<Blinking> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if desktop.split(':').any(|d| d.eq_ignore_ascii_case("KDE")) {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        let kdeglobals = std::fs::read_to_string(config.join("kdeglobals")).unwrap_or_default();
        return Some(BlinkRate::kde(&kdeglobals));
    }
    let out = Command::new("gsettings")
        .args(["list-recursively", "org.gnome.desktop.interface"])
        .output()
        .ok()
        .filter(|out| out.status.success())?;
    BlinkRate::gnome(&String::from_utf8_lossy(&out.stdout))
}

/// The unit the person measures in until they choose one: inches where
/// the system's measurements are US customary, centimetres elsewhere.
/// Windows says so itself; elsewhere the region of its locale says. On
/// Linux that's the locale measurements are in, LC_MEASUREMENT, which
/// LC_ALL overrides and LANG stands in for; on macOS the person's first
/// language's.
pub(crate) fn units() -> chrome::Unit {
    if let Some(unit) = windows_units() {
        return unit;
    }
    let posix = if cfg!(target_os = "macos") {
        None
    } else {
        ["LC_ALL", "LC_MEASUREMENT", "LANG"]
            .into_iter()
            .find_map(|name| std::env::var(name).ok().filter(|v| !v.is_empty()))
    };
    posix
        .or_else(sys_locale::get_locale)
        .map_or(chrome::Unit::Centimetre, |locale| {
            app::units_of_locale(&locale)
        })
}

/// Windows' own measurement system: `iMeasure` in
/// `HKCU\Control Panel\International`, "0" metric and "1" US.
#[cfg(target_os = "windows")]
fn windows_units() -> Option<chrome::Unit> {
    let international = windows_registry::CURRENT_USER
        .open(r"Control Panel\International")
        .ok()?;
    match international.get_string("iMeasure").ok()?.trim() {
        "0" => Some(chrome::Unit::Centimetre),
        "1" => Some(chrome::Unit::Inch),
        _ => None,
    }
}

#[cfg(not(target_os = "windows"))]
fn windows_units() -> Option<chrome::Unit> {
    None
}
