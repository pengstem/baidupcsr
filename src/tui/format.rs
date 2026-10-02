use std::borrow::Cow;
use std::sync::OnceLock;
use std::time::Duration;

use time::{OffsetDateTime, UtcOffset, macros::format_description};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

static LOCAL_OFFSET: OnceLock<UtcOffset> = OnceLock::new();

/// Capture the local UTC offset. Must run before other threads start, as
/// reading it later is refused on some platforms.
pub fn init_local_offset() {
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let _ = LOCAL_OFFSET.set(offset);
}

pub fn local_time(unix_seconds: i64) -> String {
    let offset = LOCAL_OFFSET.get().copied().unwrap_or(UtcOffset::UTC);
    OffsetDateTime::from_unix_timestamp(unix_seconds)
        .ok()
        .and_then(|time| {
            time.to_offset(offset)
                .format(format_description!("[year]-[month]-[day] [hour]:[minute]"))
                .ok()
        })
        .unwrap_or_else(|| "—".to_string())
}

/// Sizes to three significant figures: `"512 B"`, `"7.81 GiB"`, `"143 MiB"`.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KiB", "MiB", "GiB", "TiB", "PiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let digits = if value < 10.0 {
        2
    } else if value < 100.0 {
        1
    } else {
        0
    };
    format!("{value:.digits$} {}", UNITS[unit])
}

pub fn duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Cut `text` to at most `width` cells, ending in `…` when shortened.
pub fn truncate(text: &str, width: usize) -> Cow<'_, str> {
    if text.width() <= width {
        return Cow::Borrowed(text);
    }
    if width == 0 {
        return Cow::Borrowed("");
    }
    let mut used = 0;
    let mut out = String::new();
    for ch in text.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if used + ch_width > width - 1 {
            break;
        }
        used += ch_width;
        out.push(ch);
    }
    out.push('…');
    Cow::Owned(out)
}

/// The `width`-cell window of `text` starting `start` cells in.
pub fn window(text: &str, start: usize, width: usize) -> String {
    let mut skipped = 0;
    let mut used = 0;
    let mut out = String::new();
    for ch in text.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if skipped < start {
            skipped += ch_width;
            continue;
        }
        if used + ch_width > width {
            break;
        }
        used += ch_width;
        out.push(ch);
    }
    out
}

/// Pad with spaces to exactly `width` cells (after truncating).
pub fn fit(text: &str, width: usize) -> String {
    let text = truncate(text, width);
    let pad = width.saturating_sub(text.width());
    format!("{text}{}", " ".repeat(pad))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_have_three_significant_figures() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(1536), "1.50 KiB");
        assert_eq!(size(150 * 1024 * 1024), "150 MiB");
        assert_eq!(size(8_390_226_165), "7.81 GiB");
    }

    #[test]
    fn truncation_respects_wide_characters() {
        assert_eq!(truncate("绿灯军团", 5), "绿灯…");
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(fit("ab", 4), "ab  ");
        assert_eq!(window("abcdef", 2, 3), "cde");
    }

    #[test]
    fn durations() {
        assert_eq!(duration(Duration::from_secs(72)), "1:12");
        assert_eq!(duration(Duration::from_secs(3723)), "1:02:03");
    }
}
