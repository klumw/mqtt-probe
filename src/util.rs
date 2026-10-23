//! Small formatting helpers shared by the model and the UI.

use chrono::{DateTime, Local, Utc};
use rust_i18n::t;

pub fn format_bytes(bytes: u64) -> String {
    let value = bytes as f64;
    if bytes < 1024 {
        t!("util.bytes", bytes = bytes).to_string()
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", value / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", value / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GiB", value / (1024.0 * 1024.0 * 1024.0))
    }
}

pub fn format_count(count: u64) -> String {
    let separator = match rust_i18n::locale().as_ref() {
        "de" => '.',
        "fr" => '\u{202f}',
        _ => ',',
    };
    group_digits(count, separator)
}

fn group_digits(count: u64, separator: char) -> String {
    let digits = count.to_string();
    let mut result = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            result.push(separator);
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}

pub fn format_duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let secs = seconds % 60;

    if days > 0 {
        format!("{days}T {hours:02}:{minutes:02}")
    } else if hours > 0 {
        format!("{hours:02}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes:02}:{secs:02}")
    }
}

pub fn truncate_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let mut result: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        result.push('…');
    }
    result
}

/// A one-line, human readable representation of a payload.
pub fn payload_preview(payload: &[u8], max_chars: usize) -> String {
    if payload.is_empty() {
        return t!("common.empty").to_string();
    }

    let text = String::from_utf8_lossy(payload);
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized = if normalized.is_empty() {
        text.replace(['\r', '\n', '\t'], " ")
    } else {
        normalized
    };

    truncate_chars(&normalized, max_chars)
}

pub fn hex_dump(bytes: &[u8], max_bytes: usize) -> String {
    use std::fmt::Write as _;

    let shown = bytes.len().min(max_bytes);
    let mut output = String::new();
    for (line, chunk) in bytes[..shown].chunks(16).enumerate() {
        let offset = line * 16;
        let _ = write!(output, "{offset:08x}  ");

        for index in 0..16 {
            if let Some(byte) = chunk.get(index) {
                let _ = write!(output, "{byte:02x} ");
            } else {
                output.push_str("   ");
            }
            if index == 7 {
                output.push(' ');
            }
        }

        output.push(' ');
        output.push('|');
        for byte in chunk {
            let ch = if byte.is_ascii_graphic() || *byte == b' ' {
                *byte as char
            } else {
                '.'
            };
            output.push(ch);
        }
        output.push('|');
        output.push('\n');
    }

    if bytes.len() > shown {
        let _ = writeln!(
            output,
            "{}",
            t!("util.more_bytes", bytes = bytes.len() - shown)
        );
    }
    output
}

pub fn format_time(timestamp: &DateTime<Local>, utc: bool, with_date: bool) -> String {
    let pattern = if with_date {
        "%Y-%m-%d %H:%M:%S%.3f"
    } else {
        "%H:%M:%S%.3f"
    };
    if utc {
        format!("{} UTC", timestamp.with_timezone(&Utc).format(pattern))
    } else {
        timestamp.format(pattern).to_string()
    }
}

pub fn human_qos(qos: u8) -> String {
    match qos {
        0 => t!("util.qos0").to_string(),
        1 => t!("util.qos1").to_string(),
        2 => t!("util.qos2").to_string(),
        _ => t!("util.qos_unknown").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_are_formatted_with_units() {
        assert_eq!(format_bytes(512), "512 bytes");
        assert_eq!(format_bytes(2048), "2.0 KiB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MiB");
    }

    #[test]
    fn counts_use_a_locale_specific_grouping_separator() {
        assert_eq!(group_digits(0, ','), "0");
        assert_eq!(group_digits(1_234_567, ','), "1,234,567");
        assert_eq!(group_digits(1_234_567, '.'), "1.234.567");
    }

    #[test]
    fn previews_are_shortened() {
        assert_eq!(payload_preview(b"", 20), "(empty)");
        assert_eq!(payload_preview(b"hello\nworld", 40), "hello world");
        assert_eq!(payload_preview(b"abcdef", 3), "abc…");
    }

    #[test]
    fn hex_dump_contains_offset_ascii_and_truncation_note() {
        let dump = hex_dump(b"Hello MQTT", 16);
        assert!(dump.starts_with("00000000"));
        assert!(dump.contains("Hello MQTT"));

        let truncated = hex_dump(&[0u8; 40], 16);
        assert!(truncated.contains("24 more bytes"));
    }

    #[test]
    fn durations_are_formatted() {
        assert_eq!(format_duration(5), "00:05");
        assert_eq!(format_duration(65), "01:05");
        assert_eq!(format_duration(3_665), "01:01:05");
    }
}
