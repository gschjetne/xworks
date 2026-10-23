//! The caret's blinking, at the system's rate (project owner, 2026-10-24,
//! review ydqhfds0): steady while the person types or moves it and for a
//! pause after, as systems have it, then gone and back in turn, and
//! steady again once they've been idle a while, where the system stops
//! it blinking (Windows and GNOME do).
//!
//! Nothing here reads a clock: the shell says when things happen, as how
//! long it's been since it started, and asks whether the caret shows and
//! when that next changes. A window shows its caret unless the shell says
//! otherwise, so headless sessions and `shot` never blink.
//!
//! The rate comes from the system's own settings, which the shell reads
//! and these functions make sense of: Windows' registry, GNOME's
//! settings, KDE's `kdeglobals`, and macOS's user defaults.

use std::time::Duration;

/// How the system has the caret blink.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blinking {
    /// It doesn't: it shows steadily.
    Never,
    /// It does, at this rate.
    At(BlinkRate),
}

/// How a caret blinks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlinkRate {
    /// How long it shows after the person types or moves it, before it
    /// first goes.
    pub pause: Duration,
    /// How long it shows each blink.
    pub on: Duration,
    /// How long it's gone each blink.
    pub off: Duration,
    /// How long after the person last typed or moved it the caret stops
    /// blinking and shows steadily, at the end of a blink; none if it
    /// blinks on.
    pub stop: Option<Duration>,
}

impl BlinkRate {
    /// Where the system doesn't say: Windows' default, 530 milliseconds
    /// shown and as long gone, without stopping.
    pub const DEFAULT: BlinkRate = BlinkRate::even(530, None);

    /// Shown and gone `ms` milliseconds each, and shown that long after
    /// the person types, as Windows, Qt and AppKit have it.
    const fn even(ms: u64, stop: Option<Duration>) -> BlinkRate {
        let half = Duration::from_millis(ms);
        BlinkRate {
            pause: half,
            on: half,
            off: half,
            stop,
        }
    }

    /// Windows' rate, from `HKCU\Control Panel\Desktop`: `CursorBlinkRate`,
    /// the milliseconds the caret shows and is gone each, as text, "-1"
    /// for never; and `CaretTimeout`, the milliseconds after which it
    /// stops, where it's set.
    pub fn windows(rate: &str, timeout: Option<u32>) -> Option<Blinking> {
        let ms: i64 = rate.trim().parse().ok()?;
        if ms < 0 {
            return Some(Blinking::Never);
        }
        let stop = timeout.map(|t| Duration::from_millis(u64::from(t)));
        Some(Blinking::At(BlinkRate::even(ms.max(1) as u64, stop)))
    }

    /// GNOME's rate, from `gsettings list-recursively
    /// org.gnome.desktop.interface`: `cursor-blink`, whether it blinks;
    /// `cursor-blink-time`, a whole blink in milliseconds; and
    /// `cursor-blink-timeout`, the seconds after which it stops. As GTK
    /// blinks: shown two thirds of a blink and gone one, and shown a whole
    /// blink after the person types.
    pub fn gnome(listing: &str) -> Option<Blinking> {
        let value = |key: &str| {
            listing.lines().find_map(|line| {
                let mut words = line.split_whitespace();
                (words.next()? == "org.gnome.desktop.interface" && words.next()? == key)
                    .then(|| words.last())
                    .flatten()
            })
        };
        let time: u64 = value("cursor-blink-time")?.parse().ok()?;
        if value("cursor-blink") == Some("false") || time == 0 {
            return Some(Blinking::Never);
        }
        let stop = value("cursor-blink-timeout")
            .and_then(|s| s.parse::<i64>().ok())
            .filter(|&s| s < i64::from(i32::MAX) / 1000)
            .map(|s| Duration::from_secs(s.max(0) as u64));
        Some(Blinking::At(BlinkRate {
            pause: Duration::from_millis(time),
            on: Duration::from_millis(time * 2 / 3),
            off: Duration::from_millis(time / 3),
            stop,
        }))
    }

    /// KDE's rate, from `kdeglobals`: `CursorBlinkRate` in its `[KDE]`
    /// group, a whole blink in milliseconds, 0 for never; a thousand where
    /// it isn't set, as Qt has it. Qt shows and hides the caret half a
    /// blink each.
    pub fn kde(kdeglobals: &str) -> Blinking {
        let mut group = "";
        let mut rate = 1000;
        for line in kdeglobals.lines().map(str::trim) {
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                group = name;
            } else if group == "KDE"
                && let Some((key, value)) = line.split_once('=')
                && key.trim() == "CursorBlinkRate"
                && let Ok(ms) = value.trim().parse::<i64>()
            {
                rate = ms;
            }
        }
        if rate <= 0 {
            return Blinking::Never;
        }
        Blinking::At(BlinkRate::even((rate as u64 / 2).max(1), None))
    }

    /// macOS's rate, from its user defaults
    /// `NSTextInsertionPointBlinkPeriodOn` and `...Off`, in milliseconds,
    /// where either is set: one set alone stands for both.
    pub fn mac(on: Option<f64>, off: Option<f64>) -> Option<Blinking> {
        let ms = |v: f64| Duration::from_millis(v.clamp(1.0, 60_000.0) as u64);
        let (on, off) = match (on, off) {
            (None, None) => return None,
            (Some(on), None) => (on, on),
            (None, Some(off)) => (off, off),
            (Some(on), Some(off)) => (on, off),
        };
        Some(Blinking::At(BlinkRate {
            pause: ms(on),
            on: ms(on),
            off: ms(off),
            stop: None,
        }))
    }
}

/// A caret's blinking: how it blinks, and when the person last typed or
/// moved it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blink {
    blinking: Blinking,
    since: Duration,
}

impl Blink {
    /// A caret that blinks as `blinking` says, last moved at the start.
    pub fn new(blinking: Blinking) -> Blink {
        Blink {
            blinking,
            since: Duration::ZERO,
        }
    }

    /// It blinks as `blinking` says from now on.
    pub fn set(&mut self, blinking: Blinking) {
        self.blinking = blinking;
    }

    /// The person typed or moved the caret at `now`: it shows, steadily,
    /// for a pause.
    pub fn moved(&mut self, now: Duration) {
        self.since = now;
    }

    /// When it stops blinking and shows steadily, from when it was last
    /// moved: at the end of the first blink to end after the rate's stop.
    fn stops(rate: &BlinkRate) -> Option<Duration> {
        let stop = rate.stop?;
        let period = rate.on + rate.off;
        let blinking = stop.saturating_sub(rate.pause);
        let blinks = blinking.as_nanos().div_ceil(period.as_nanos().max(1));
        Some(rate.pause + period * u32::try_from(blinks).unwrap_or(u32::MAX))
    }

    /// Whether the caret shows at `now`, and when that next changes, if
    /// it does.
    fn at(&self, now: Duration) -> (bool, Option<Duration>) {
        let Blinking::At(rate) = self.blinking else {
            return (true, None);
        };
        let t = now.saturating_sub(self.since);
        let stops = Blink::stops(&rate);
        if stops.is_some_and(|stops| t >= stops || stops <= rate.pause) {
            return (true, None);
        }
        if t < rate.pause {
            return (true, Some(self.since + rate.pause));
        }
        // Each blink after the pause: gone, then shown; the last ends
        // shown, as it stays.
        let period = (rate.on + rate.off).as_nanos().max(1);
        let into = (t - rate.pause).as_nanos();
        let phase = into % period;
        let start = rate.pause + nanos(into - phase);
        if phase < rate.off.as_nanos() {
            return (false, Some(self.since + start + rate.off));
        }
        let end = start + nanos(period);
        let next = stops
            .is_none_or(|stops| end < stops)
            .then(|| self.since + end);
        (true, next)
    }

    /// Whether the caret shows at `now`.
    pub fn shown(&self, now: Duration) -> bool {
        self.at(now).0
    }

    /// When, after `now`, the caret next shows or goes; none if it stays
    /// as it is until it's moved.
    pub fn next(&self, now: Duration) -> Option<Duration> {
        self.at(now).1
    }
}

/// `n` nanoseconds.
fn nanos(n: u128) -> Duration {
    Duration::from_nanos(u64::try_from(n).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Whether the caret shows at each of `times`, in milliseconds.
    fn shown(blink: &Blink, times: &[u64]) -> Vec<bool> {
        times.iter().map(|&t| blink.shown(ms(t))).collect()
    }

    /// The caret shows while the person types, and for a pause after;
    /// then it blinks, gone and shown in turn; and where the system stops
    /// it, it shows steadily after a while idle, until they type again.
    #[test]
    fn the_caret_shows_while_typing_then_blinks_then_rests() {
        // Windows: 530 ms each way, stopping after 5 seconds.
        let rate = BlinkRate::windows("530", Some(5000)).unwrap();
        let mut blink = Blink::new(rate);
        // Typing, a key every 200 ms: it shows throughout.
        for t in (0..=2000).step_by(200) {
            blink.moved(ms(t));
            assert!(blink.shown(ms(t)) && blink.shown(ms(t + 199)), "at {t}");
        }
        // The last key at 2 s; shown till 2.53, gone till 3.06, back
        // till 3.59, and so on.
        assert_eq!(
            shown(&blink, &[2529, 2530, 3059, 3060, 3589, 3590]),
            [true, false, false, true, true, false]
        );
        assert_eq!(blink.next(ms(2100)), Some(ms(2530)));
        assert_eq!(blink.next(ms(2600)), Some(ms(3060)));
        assert_eq!(blink.next(ms(3100)), Some(ms(3590)));
        // Five seconds on it stops at the end of a blink, shown: after
        // 0.53 s and five whole blinks of 1.06 s, at 7.83 s. The last
        // blink is gone from 6.77 s and back from 7.3 s, to stay.
        assert_eq!(
            shown(&blink, &[6769, 6770, 7299, 7300, 7829, 7830]),
            [true, false, false, true, true, true]
        );
        assert_eq!(blink.next(ms(6800)), Some(ms(7300)));
        assert_eq!(blink.next(ms(7400)), None);
        assert!(shown(&blink, &[7830, 9000, 60_000]).iter().all(|s| *s));
        assert_eq!(blink.next(ms(7830)), None);
        // A key brings it back to blinking.
        blink.moved(ms(60_000));
        assert_eq!(blink.next(ms(60_000)), Some(ms(60_530)));
        assert!(!blink.shown(ms(60_600)));
    }

    #[test]
    fn gnome_blinks_as_gtk_does() {
        let listing = "org.gnome.desktop.interface cursor-blink true\n\
                       org.gnome.desktop.interface cursor-blink-time 1200\n\
                       org.gnome.desktop.interface cursor-blink-timeout 10\n\
                       org.gnome.desktop.interface cursor-size 24\n";
        let Some(Blinking::At(rate)) = BlinkRate::gnome(listing) else {
            panic!("a rate");
        };
        assert_eq!(
            rate,
            BlinkRate {
                pause: ms(1200),
                on: ms(800),
                off: ms(400),
                stop: Some(Duration::from_secs(10)),
            }
        );
        let blink = Blink::new(Blinking::At(rate));
        // Shown 1.2 s, gone 0.4, shown 0.8; stopped after ten seconds,
        // at the end of the eighth blink, which is gone from 9.6 s and
        // back from 10 s, to stay.
        assert_eq!(
            shown(&blink, &[1199, 1200, 1599, 1600, 2399, 2400]),
            [true, false, false, true, true, false]
        );
        assert_eq!(
            shown(&blink, &[9599, 9600, 9999, 10_000, 10_800, 20_000]),
            [true, false, false, true, true, true]
        );
        assert_eq!(blink.next(ms(9700)), Some(ms(10_000)));
        assert_eq!(blink.next(ms(10_100)), None);
        // With no time to blink, it never does.
        let mut never = rate;
        never.stop = Some(Duration::ZERO);
        let never = Blink::new(Blinking::At(never));
        assert!(shown(&never, &[0, 1300, 1700]).iter().all(|s| *s));
        assert_eq!(never.next(ms(0)), None);
        let off = listing.replace("cursor-blink true", "cursor-blink false");
        assert_eq!(BlinkRate::gnome(&off), Some(Blinking::Never));
        assert_eq!(BlinkRate::gnome("No such schema"), None);
    }

    #[test]
    fn each_system_says_its_rate() {
        assert_eq!(BlinkRate::windows("-1", None), Some(Blinking::Never));
        assert_eq!(
            BlinkRate::windows(" 400 ", None),
            Some(Blinking::At(BlinkRate::even(400, None)))
        );
        assert_eq!(BlinkRate::windows("fast", None), None);
        assert_eq!(
            BlinkRate::kde("[General]\nCursorBlinkRate=0\n[KDE]\nCursorBlinkRate=1200\n"),
            Blinking::At(BlinkRate::even(600, None))
        );
        assert_eq!(BlinkRate::kde("[KDE]\nCursorBlinkRate=0"), Blinking::Never);
        assert_eq!(BlinkRate::kde(""), Blinking::At(BlinkRate::even(500, None)));
        assert_eq!(
            BlinkRate::mac(Some(300.0), None),
            Some(Blinking::At(BlinkRate::even(300, None)))
        );
        assert_eq!(BlinkRate::mac(None, None), None);
        // A caret that doesn't blink always shows.
        let steady = Blink::new(Blinking::Never);
        assert!(steady.shown(ms(10_000)));
        assert_eq!(steady.next(ms(0)), None);
    }
}
