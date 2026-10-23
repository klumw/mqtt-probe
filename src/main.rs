#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! MQTT Probe — a native Rust reimplementation of MQTT Explorer.
//!
//! Built with `eframe`/`egui` for the interface and `rumqttc`/`tokio`
//! for the MQTT client.

mod app;
mod config;
mod model;
mod mqtt;
mod ui;
mod util;

rust_i18n::i18n!("locales", fallback = "en");

use eframe::egui;

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MQTT Probe")
            .with_app_id("mqtt-probe")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([980.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        "MQTT Probe",
        native_options,
        Box::new(|cc| Ok(Box::new(app::MqttUiApp::new(cc)))),
    )
}
