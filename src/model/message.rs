//! A single MQTT message kept in memory.

use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Local, Utc};
use serde_json::Value;

use crate::util::payload_preview;

static NEXT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct MessageRecord {
    /// Stable identity for UI selections, independent of the history index.
    pub seq: u64,
    pub topic: String,
    pub payload: Vec<u8>,
    pub qos: u8,
    pub retain: bool,
    pub received_at: DateTime<Local>,
}

impl MessageRecord {
    pub fn new(topic: String, payload: Vec<u8>, qos: u8, retain: bool) -> Self {
        Self {
            seq: NEXT_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            topic,
            payload,
            qos,
            retain,
            received_at: Local::now(),
        }
    }

    pub fn text(&self) -> Option<&str> {
        std::str::from_utf8(&self.payload).ok()
    }

    pub fn text_lossy(&self) -> String {
        String::from_utf8_lossy(&self.payload).into_owned()
    }

    pub fn size(&self) -> usize {
        self.payload.len()
    }

    pub fn preview(&self, max_chars: usize) -> String {
        payload_preview(&self.payload, max_chars)
    }

    pub fn json(&self) -> Option<Value> {
        serde_json::from_slice::<Value>(&self.payload).ok()
    }

    pub fn is_json(&self) -> bool {
        self.json().is_some()
    }

    pub fn pretty_json(&self) -> Option<String> {
        self.json()
            .and_then(|value| serde_json::to_string_pretty(&value).ok())
    }

    /// Returns the payload as a number if it is a plain number or a JSON number.
    pub fn numeric_value(&self) -> Option<f64> {
        if let Some(Value::Number(number)) = self.json() {
            return number.as_f64().filter(|value| value.is_finite());
        }

        self.text()?
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
    }

    /// All numeric leaf values of a JSON payload, keyed by their dotted path.
    /// A plain numeric payload is exposed under the key `value`.
    pub fn numeric_fields(&self) -> Vec<(String, f64)> {
        let mut fields = Vec::new();
        match self.json() {
            Some(value) => collect_numeric(&value, "", 0, &mut fields),
            None => {
                if let Some(value) = self.numeric_value() {
                    fields.push(("value".to_owned(), value));
                }
            }
        }
        fields
    }

    /// Reads a single numeric value from the payload using a path produced by
    /// [`Self::numeric_fields`].
    pub fn numeric_at(&self, field: &str) -> Option<f64> {
        let json = self.json();
        if field == "value" {
            match &json {
                None => return self.numeric_value(),
                Some(Value::Number(number)) => {
                    return number.as_f64().filter(|value| value.is_finite());
                }
                Some(_) => {}
            }
        }
        let value = json?;
        let selected = value_at(&value, field)?;
        selected.as_f64().filter(|value| value.is_finite())
    }

    pub fn timestamp_string(&self, utc: bool) -> String {
        if utc {
            format!(
                "{} UTC",
                self.received_at.with_timezone(&Utc).format("%H:%M:%S%.3f")
            )
        } else {
            self.received_at.format("%H:%M:%S%.3f").to_string()
        }
    }

    pub fn full_timestamp_string(&self, utc: bool) -> String {
        if utc {
            format!(
                "{} UTC",
                self.received_at
                    .with_timezone(&Utc)
                    .format("%Y-%m-%d %H:%M:%S%.3f")
            )
        } else {
            self.received_at.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
        }
    }
}

fn collect_numeric(value: &Value, prefix: &str, depth: usize, out: &mut Vec<(String, f64)>) {
    const MAX_FIELDS: usize = 64;
    const MAX_DEPTH: usize = 6;

    if out.len() >= MAX_FIELDS || depth > MAX_DEPTH {
        return;
    }

    match value {
        Value::Number(number) => {
            if let Some(value) = number.as_f64().filter(|value| value.is_finite()) {
                out.push((
                    if prefix.is_empty() {
                        "value".to_owned()
                    } else {
                        prefix.to_owned()
                    },
                    value,
                ));
            }
        }
        Value::Object(map) => {
            for (key, value) in map {
                let key = escape_key(key);
                let path = if prefix.is_empty() {
                    key
                } else {
                    format!("{prefix}.{key}")
                };
                collect_numeric(value, &path, depth + 1, out);
            }
        }
        Value::Array(items) => {
            for (index, value) in items.iter().enumerate() {
                let path = format!("{prefix}[{index}]");
                collect_numeric(value, &path, depth + 1, out);
            }
        }
        _ => {}
    }
}

/// Escapes dots and backslashes so a JSON key remains a single path segment.
fn escape_key(key: &str) -> String {
    key.replace('\\', "\\\\").replace('.', "\\.")
}

/// Splits an escaped field path into its unescaped segments.
fn split_path(path: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = path.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if let Some(escaped) = chars.next() {
                    current.push(escaped);
                }
            }
            '.' => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// Turns an escaped field path into a human-readable label.
pub fn field_label(path: &str) -> String {
    split_path(path).join(".")
}

fn value_at<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = root;
    for part in split_path(path) {
        let (name, indices) = split_indices(&part);
        if !name.is_empty() {
            current = current.get(name)?;
        }
        for index in indices {
            current = current.get(index)?;
        }
    }
    Some(current)
}

fn split_indices(part: &str) -> (&str, Vec<usize>) {
    let Some(start) = part.find('[') else {
        return (part, Vec::new());
    };
    let name = &part[..start];
    let mut indices = Vec::new();
    for chunk in part[start..].split('[').skip(1) {
        if let Some(end) = chunk.find(']')
            && let Ok(index) = chunk[..end].parse::<usize>()
        {
            indices.push(index);
        }
    }
    (name, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_value_from_plain_and_json() {
        let plain = MessageRecord::new("a".into(), b"21.5".to_vec(), 0, false);
        assert_eq!(plain.numeric_value(), Some(21.5));

        let json = MessageRecord::new("a".into(), b"42".to_vec(), 0, false);
        assert_eq!(json.numeric_value(), Some(42.0));

        let text = MessageRecord::new("a".into(), b"hello".to_vec(), 0, false);
        assert_eq!(text.numeric_value(), None);
    }

    #[test]
    fn nested_numeric_fields_are_addressable() {
        let message = MessageRecord::new(
            "sensor".into(),
            br#"{"temperature": 21.4, "nested": {"humidity": 55}, "values": [1, 2]}"#.to_vec(),
            0,
            false,
        );

        let fields = message.numeric_fields();
        assert!(fields.iter().any(|(key, _)| key == "temperature"));
        assert!(fields.iter().any(|(key, _)| key == "nested.humidity"));
        assert!(fields.iter().any(|(key, _)| key == "values[1]"));

        assert_eq!(message.numeric_at("nested.humidity"), Some(55.0));
        assert_eq!(message.numeric_at("values[1]"), Some(2.0));
    }

    #[test]
    fn pretty_json_roundtrip() {
        let message = MessageRecord::new("a".into(), br#"{"ok":true}"#.to_vec(), 0, false);
        assert!(message.pretty_json().unwrap().contains("\"ok\": true"));
    }

    #[test]
    fn sequence_numbers_are_unique() {
        let first = MessageRecord::new("a".into(), b"1".to_vec(), 0, false);
        let second = MessageRecord::new("a".into(), b"2".to_vec(), 0, false);
        assert_ne!(first.seq, second.seq);
    }

    #[test]
    fn json_keys_with_dots_remain_addressable() {
        let message = MessageRecord::new(
            "air".into(),
            br#"{"VINDRIKTING.PM2.5: 8": 8, "nested": {"a.b": 3}}"#.to_vec(),
            0,
            false,
        );

        let fields = message.numeric_fields();
        assert!(
            fields
                .iter()
                .any(|(key, value)| key == "VINDRIKTING\\.PM2\\.5: 8" && *value == 8.0)
        );
        assert!(
            fields
                .iter()
                .any(|(key, value)| key == "nested.a\\.b" && *value == 3.0)
        );
        assert_eq!(message.numeric_at("VINDRIKTING\\.PM2\\.5: 8"), Some(8.0));
        assert_eq!(message.numeric_at("nested.a\\.b"), Some(3.0));
        assert_eq!(
            field_label("VINDRIKTING\\.PM2\\.5: 8"),
            "VINDRIKTING.PM2.5: 8"
        );
        assert_eq!(field_label("nested.a\\.b"), "nested.a.b");
    }

    #[test]
    fn json_key_named_value_does_not_shadow_plain_number_handling() {
        let message = MessageRecord::new("a".into(), br#"{"value": 42}"#.to_vec(), 0, false);
        assert_eq!(message.numeric_fields(), vec![("value".to_owned(), 42.0)]);
        assert_eq!(message.numeric_at("value"), Some(42.0));

        let plain = MessageRecord::new("a".into(), b"7.5".to_vec(), 0, false);
        assert_eq!(plain.numeric_at("value"), Some(7.5));
    }
}
