//! Persistent application configuration: connection profiles, UI settings and
//! the publish history.

use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use rust_i18n::t;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

pub const DEFAULT_SUBSCRIBE_TOPIC: &str = "#";
pub const DEFAULT_MAX_HISTORY: usize = 200;
pub const MAX_PUBLISH_HISTORY: usize = 20;
pub const DEFAULT_WEBSOCKET_PATH: &str = "/mqtt";

/// The transport used to reach the broker. The four variants map directly to
/// the persisted `websocket` and `tls` flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Tcp,
    Tls,
    Ws,
    Wss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    English,
    Korean,
    Chinese,
    French,
    German,
    Spanish,
}

impl Language {
    pub const ALL: [Language; 6] = [
        Language::English,
        Language::Korean,
        Language::Chinese,
        Language::French,
        Language::German,
        Language::Spanish,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Korean => "ko",
            Self::Chinese => "zh",
            Self::French => "fr",
            Self::German => "de",
            Self::Spanish => "es",
        }
    }

    /// The language's own name, shown untranslated in the picker.
    pub fn native_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Korean => "한국어",
            Self::Chinese => "中文",
            Self::French => "Français",
            Self::German => "Deutsch",
            Self::Spanish => "Español",
        }
    }

    /// Accepts ISO codes and English language names. Unknown values fall back
    /// to English so a malformed config never prevents the app from starting.
    pub fn from_code(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "ko" | "korean" => Self::Korean,
            "zh" | "zh-cn" | "zh-hans" | "chinese" => Self::Chinese,
            "fr" | "french" => Self::French,
            "de" | "german" => Self::German,
            "es" | "spanish" => Self::Spanish,
            _ => Self::English,
        }
    }
}

impl Serialize for Language {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.code())
    }
}

impl<'de> Deserialize<'de> for Language {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::from_code(&String::deserialize(deserializer)?))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub profiles: Vec<ConnectionProfile>,
    pub settings: AppSettings,
    pub publish_history: Vec<PublishHistoryEntry>,
    /// Last displayed topic trees, saved when the application exits.
    pub topic_snapshots: Vec<ConnectionSnapshot>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            profiles: vec![ConnectionProfile::default()],
            settings: AppSettings::default(),
            publish_history: Vec::new(),
            topic_snapshots: Vec::new(),
        }
    }
}

impl AppConfig {
    /// Marks `id` as the single profile that connects automatically at
    /// startup and clears the flag on every other profile.
    pub fn set_auto_connect_profile(&mut self, id: Uuid) {
        for profile in &mut self.profiles {
            profile.auto_connect = profile.id == id;
        }
    }

    /// Keeps at most one auto-connect profile. Configurations written by
    /// older versions or edited by hand may contain several; the first
    /// profile in list order wins.
    pub fn normalize_auto_connect(&mut self) {
        let mut found = false;
        for profile in &mut self.profiles {
            if profile.auto_connect {
                if found {
                    log::warn!(
                        "Multiple profiles were marked for startup connection; disabling auto-connect for \"{}\"",
                        profile.name
                    );
                    profile.auto_connect = false;
                } else {
                    found = true;
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ConnectionSnapshot {
    pub profile_id: Uuid,
    pub topics: Vec<SavedTopic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedTopic {
    pub topic: String,
    /// Base64-encoded last payload, so binary data survives the round trip.
    pub payload_base64: String,
    pub qos: u8,
    pub retain: bool,
    pub received_at: DateTime<Local>,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConnectionProfile {
    pub id: Uuid,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    /// Use MQTT over WebSockets instead of raw TCP. Combined with `tls` this
    /// selects `ws` (false) or `wss` (true).
    pub websocket: bool,
    /// HTTP path of the WebSocket endpoint (HiveMQ default: `/mqtt`).
    pub websocket_path: String,
    /// Optional path to an additional PEM/DER CA certificate for TLS.
    pub ca_cert_path: String,
    /// Optional path to a PEM/DER client certificate chain for mutual TLS.
    pub client_cert_path: String,
    /// Optional path to the unencrypted PEM/DER private key belonging to
    /// `client_cert_path`.
    pub client_key_path: String,
    pub client_id: String,
    pub username: String,
    pub password: String,
    /// Opt-in: store the password in the plain-text config file.
    pub save_password: bool,
    pub keep_alive_secs: u64,
    pub clean_session: bool,
    pub subscribe_topic: String,
    pub subscribe_qos: u8,
    pub auto_connect: bool,
}

impl Default for ConnectionProfile {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: t!("connections.default_profile_name").to_string(),
            host: "localhost".to_owned(),
            port: 1883,
            tls: false,
            websocket: false,
            websocket_path: DEFAULT_WEBSOCKET_PATH.to_owned(),
            ca_cert_path: String::new(),
            client_cert_path: String::new(),
            client_key_path: String::new(),
            client_id: new_client_id(),
            username: String::new(),
            password: String::new(),
            save_password: false,
            keep_alive_secs: 60,
            clean_session: true,
            subscribe_topic: DEFAULT_SUBSCRIBE_TOPIC.to_owned(),
            subscribe_qos: 1,
            auto_connect: false,
        }
    }
}

impl ConnectionProfile {
    pub fn transport_kind(&self) -> TransportKind {
        match (self.websocket, self.tls) {
            (false, false) => TransportKind::Tcp,
            (false, true) => TransportKind::Tls,
            (true, false) => TransportKind::Ws,
            (true, true) => TransportKind::Wss,
        }
    }

    pub fn set_transport_kind(&mut self, kind: TransportKind) {
        self.websocket = matches!(kind, TransportKind::Ws | TransportKind::Wss);
        self.tls = matches!(kind, TransportKind::Tls | TransportKind::Wss);
    }

    /// A short representation used in lists and tooltips. WebSocket profiles
    /// show their full URL so the scheme and path are visible.
    pub fn endpoint(&self) -> String {
        if self.websocket {
            crate::mqtt::websocket::url_for(self)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    /// The displayed profile name. Generated default names are translated to
    /// the current UI language; custom names are kept as entered.
    pub fn display_name(&self) -> String {
        if self.name.trim().is_empty() {
            self.host.clone()
        } else if is_default_profile_name(&self.name) {
            t!("connections.default_profile_name").to_string()
        } else {
            self.name.clone()
        }
    }
}

/// Returns true for the generated default name in any supported language, so
/// profiles created under a different UI language follow language changes.
pub fn is_default_profile_name(name: &str) -> bool {
    matches!(
        name.trim(),
        "Local broker" | "Lokaler Broker" | "Broker local" | "로컬 브로커" | "本地 Broker"
    )
}

pub fn new_client_id() -> String {
    let id = Uuid::new_v4().simple().to_string();
    format!("mqtt-probe-{}", &id[..8])
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// UI language. Missing or unknown values fall back to English.
    pub language: Language,
    pub dark_mode: bool,
    pub max_history: usize,
    pub show_payload_preview: bool,
    pub search_in_payload: bool,
    pub json_pretty: bool,
    pub timestamps_utc: bool,
    pub max_display_bytes: usize,
    /// Whether the bottom publish composer was open when the app closed.
    pub publish_open: bool,
    /// Briefly brighten the topic tree whenever a message arrives.
    pub flash_tree_on_update: bool,
    /// How long the text highlighting remains visible, in seconds.
    pub flash_duration_secs: f32,
    /// After reconnecting, restore only the topics that were visible when the
    /// application last exited and ignore the rest of the retained backlog.
    pub restore_previous_topics: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: Language::English,
            dark_mode: true,
            max_history: DEFAULT_MAX_HISTORY,
            show_payload_preview: true,
            search_in_payload: false,
            json_pretty: true,
            timestamps_utc: false,
            max_display_bytes: 64 * 1024,
            publish_open: true,
            flash_tree_on_update: true,
            flash_duration_secs: 3.0,
            restore_previous_topics: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublishHistoryEntry {
    pub topic: String,
    pub payload: String,
    pub qos: u8,
    pub retain: bool,
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

impl Default for ConfigStore {
    fn default() -> Self {
        Self::new()
    }
}

const CONFIG_DIR_NAME: &str = "mqtt-probe";

impl ConfigStore {
    pub fn new() -> Self {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            path: base.join(CONFIG_DIR_NAME).join("config.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> AppConfig {
        match std::fs::read_to_string(&self.path) {
            Ok(raw) => parse_config(&raw),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => AppConfig::default(),
            Err(err) => {
                log::warn!("Could not load the configuration: {err}");
                AppConfig::default()
            }
        }
    }

    pub fn save(&self, config: &AppConfig) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
        }

        // Never persist passwords unless the user explicitly opted in.
        let mut sanitized = config.clone();
        for profile in &mut sanitized.profiles {
            if !profile.save_password {
                profile.password.clear();
            }
        }

        let json = serde_json::to_string_pretty(&sanitized)?;
        let tmp = self.path.with_extension("json.tmp");
        let _ = std::fs::remove_file(&tmp);

        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

fn parse_config(raw: &str) -> AppConfig {
    match serde_json::from_str::<AppConfig>(raw) {
        Ok(mut config) => {
            config.normalize_auto_connect();
            config
        }
        Err(err) => {
            log::warn!("Could not read the configuration: {err}");
            AppConfig::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_use_iso_codes_and_fall_back_to_english() {
        assert_eq!(Language::from_code("en"), Language::English);
        assert_eq!(Language::from_code("de"), Language::German);
        assert_eq!(Language::from_code("fr"), Language::French);
        assert_eq!(Language::from_code("ko"), Language::Korean);
        assert_eq!(Language::from_code("zh"), Language::Chinese);
        assert_eq!(Language::from_code("zh-CN"), Language::Chinese);
        assert_eq!(Language::from_code("es"), Language::Spanish);
        assert_eq!(Language::from_code("spanish"), Language::Spanish);
        assert_eq!(Language::from_code("Klingon"), Language::English);
        assert_eq!(serde_json::to_string(&Language::French).unwrap(), "\"fr\"");
        assert_eq!(
            serde_json::from_str::<Language>("\"ko\"").unwrap(),
            Language::Korean
        );
    }

    #[test]
    fn generated_profile_names_follow_the_ui_language() {
        for name in [
            "Local broker",
            "Lokaler Broker",
            "Broker local",
            "로컬 브로커",
            "本地 Broker",
        ] {
            assert!(is_default_profile_name(name), "{name}");
        }
        assert!(!is_default_profile_name("Sensor gateway"));

        let profile = ConnectionProfile {
            name: "Lokaler Broker".to_owned(),
            ..ConnectionProfile::default()
        };
        assert_eq!(profile.display_name(), "Local broker");
    }

    #[test]
    fn only_one_profile_can_connect_at_startup() {
        let mut config = AppConfig::default();
        let first = config.profiles[0].id;
        let second = ConnectionProfile {
            auto_connect: true,
            ..ConnectionProfile::default()
        };
        let second_id = second.id;
        config.profiles.push(second);
        config.profiles.push(ConnectionProfile {
            auto_connect: true,
            ..ConnectionProfile::default()
        });

        config.normalize_auto_connect();
        assert!(!config.profiles[0].auto_connect);
        assert!(config.profiles[1].auto_connect);
        assert!(!config.profiles[2].auto_connect);

        config.set_auto_connect_profile(first);
        assert!(config.profiles[0].auto_connect);
        assert!(!config.profiles[1].auto_connect);
        assert_eq!(
            config
                .profiles
                .iter()
                .filter(|profile| profile.auto_connect)
                .count(),
            1
        );

        config.set_auto_connect_profile(second_id);
        assert!(!config.profiles[0].auto_connect);
        assert!(config.profiles[1].auto_connect);
    }

    #[test]
    fn loading_keeps_only_one_auto_connect_profile() {
        let dir =
            std::env::temp_dir().join(format!("mqtt-probe-config-test-{}", uuid::Uuid::new_v4()));
        let store = ConfigStore {
            path: dir.join("config.json"),
        };
        let mut config = AppConfig::default();
        config.profiles[0].auto_connect = true;
        config.profiles.push(ConnectionProfile {
            auto_connect: true,
            ..ConnectionProfile::default()
        });
        store.save(&config).unwrap();

        let loaded = store.load();
        assert!(loaded.profiles[0].auto_connect);
        assert!(!loaded.profiles[1].auto_connect);
    }

    #[test]
    fn transport_kind_maps_to_the_persisted_flags() {
        let mut profile = ConnectionProfile::default();
        assert_eq!(profile.transport_kind(), TransportKind::Tcp);

        profile.set_transport_kind(TransportKind::Tls);
        assert!(profile.tls);
        assert!(!profile.websocket);
        assert_eq!(profile.transport_kind(), TransportKind::Tls);

        profile.set_transport_kind(TransportKind::Ws);
        assert!(!profile.tls);
        assert!(profile.websocket);
        assert_eq!(profile.transport_kind(), TransportKind::Ws);

        profile.set_transport_kind(TransportKind::Wss);
        assert!(profile.tls);
        assert!(profile.websocket);
        assert_eq!(profile.transport_kind(), TransportKind::Wss);
    }

    #[test]
    fn websocket_fields_roundtrip_and_default_to_off() {
        let mut config = AppConfig::default();
        assert!(!config.profiles[0].websocket);
        assert_eq!(config.profiles[0].websocket_path, DEFAULT_WEBSOCKET_PATH);

        config.profiles[0].set_transport_kind(TransportKind::Wss);
        config.profiles[0].websocket_path = "/custom".to_owned();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert!(parsed.profiles[0].websocket);
        assert!(parsed.profiles[0].tls);
        assert_eq!(parsed.profiles[0].websocket_path, "/custom");
    }

    #[test]
    fn websocket_endpoint_contains_scheme_and_path() {
        let mut profile = ConnectionProfile {
            host: "broker.example".to_owned(),
            port: 8000,
            websocket: true,
            websocket_path: "/mqtt".to_owned(),
            ..ConnectionProfile::default()
        };
        assert_eq!(profile.endpoint(), "ws://broker.example:8000/mqtt");

        profile.tls = true;
        assert_eq!(profile.endpoint(), "wss://broker.example:8000/mqtt");
    }

    #[test]
    fn config_roundtrip_keeps_profiles_and_settings() {
        let mut config = AppConfig::default();
        config.settings.max_history = 42;
        config.settings.publish_open = false;
        config.settings.flash_tree_on_update = false;
        config.settings.flash_duration_secs = 4.5;
        config.settings.restore_previous_topics = false;
        config.settings.language = Language::German;
        config.profiles[0].host = "broker.example".to_owned();
        config.profiles[0].tls = true;
        config.profiles[0].ca_cert_path = "/etc/ssl/private-ca.pem".to_owned();
        config.profiles[0].client_cert_path = "/etc/ssl/client.pem".to_owned();
        config.profiles[0].client_key_path = "/etc/ssl/client-key.pem".to_owned();
        config.topic_snapshots.push(ConnectionSnapshot {
            profile_id: config.profiles[0].id,
            topics: vec![SavedTopic {
                topic: "/demo/value".to_owned(),
                payload_base64: "aGVsbG8=".to_owned(),
                qos: 1,
                retain: true,
                received_at: Local::now(),
                total: 3,
            }],
        });
        config.publish_history.push(PublishHistoryEntry {
            topic: "demo/topic".to_owned(),
            payload: "{\"ok\":true}".to_owned(),
            qos: 1,
            retain: false,
        });

        let json = serde_json::to_string(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.settings.max_history, 42);
        assert_eq!(parsed.settings.language, Language::German);
        assert!(
            serde_json::to_string(&parsed.settings.language)
                .unwrap()
                .contains("\"de\"")
        );
        assert!(!parsed.settings.publish_open);
        assert!(!parsed.settings.flash_tree_on_update);
        assert!(!parsed.settings.restore_previous_topics);
        assert!((parsed.settings.flash_duration_secs - 4.5).abs() < f32::EPSILON);
        assert_eq!(parsed.profiles[0].host, "broker.example");
        assert_eq!(parsed.profiles[0].ca_cert_path, "/etc/ssl/private-ca.pem");
        assert_eq!(parsed.profiles[0].client_cert_path, "/etc/ssl/client.pem");
        assert_eq!(
            parsed.profiles[0].client_key_path,
            "/etc/ssl/client-key.pem"
        );
        assert_eq!(parsed.publish_history.len(), 1);
        assert_eq!(parsed.topic_snapshots.len(), 1);
        assert_eq!(parsed.topic_snapshots[0].topics[0].topic, "/demo/value");
        assert_eq!(parsed.topic_snapshots[0].topics[0].total, 3);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let parsed: AppConfig =
            serde_json::from_str("{\"settings\": {\"max_history\": 7}}").unwrap();
        assert_eq!(parsed.settings.max_history, 7);
        assert_eq!(parsed.settings.language, Language::English);
        assert!(parsed.settings.dark_mode);
        assert!(parsed.settings.publish_open);
        assert!(parsed.settings.flash_tree_on_update);
        assert!(parsed.settings.restore_previous_topics);
        assert!((parsed.settings.flash_duration_secs - 3.0).abs() < f32::EPSILON);
        assert_eq!(parsed.profiles[0].port, 1883);
        assert!(!parsed.profiles[0].websocket);
        assert_eq!(parsed.profiles[0].websocket_path, DEFAULT_WEBSOCKET_PATH);
    }

    #[cfg(unix)]
    #[test]
    fn save_redacts_passwords_and_sets_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir =
            std::env::temp_dir().join(format!("mqtt-probe-config-test-{}", uuid::Uuid::new_v4()));
        let store = ConfigStore {
            path: dir.join("config.json"),
        };
        let mut config = AppConfig::default();
        config.profiles[0].password = "top-secret".to_owned();
        config.profiles[0].save_password = false;

        store.save(&config).unwrap();
        let raw = std::fs::read_to_string(store.path()).unwrap();
        assert!(!raw.contains("top-secret"));
        assert_eq!(
            std::fs::metadata(store.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );

        config.profiles[0].save_password = true;
        store.save(&config).unwrap();
        let raw = std::fs::read_to_string(store.path()).unwrap();
        assert!(raw.contains("top-secret"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
