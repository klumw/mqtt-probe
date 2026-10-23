//! Background MQTT worker.
//!
//! Each connection owns a dedicated OS thread with a single-threaded Tokio
//! runtime. The UI talks to it through an unbounded command channel and
//! receives [`WorkerEvent`]s through a standard channel. Incoming events wake
//! the UI via `egui::Context::request_repaint`.

use std::time::{Duration, Instant};

use eframe::egui;
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS, SubscribeReasonCode, Transport};
use rust_i18n::t;

use crate::config::ConnectionProfile;
use crate::model::MessageRecord;

use super::types::{Command, ConnectionHandle, ConnectionState, WorkerEvent};

const MAX_PACKET_SIZE: usize = 8 * 1024 * 1024;
const REQUEST_CAPACITY: usize = 256;
const RECONNECT_DELAY: Duration = Duration::from_millis(1_500);
const ERROR_LOG_INTERVAL: Duration = Duration::from_secs(5);

pub fn spawn(profile: ConnectionProfile, ctx: egui::Context) -> ConnectionHandle {
    let (commands, command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (event_tx, events) = std::sync::mpsc::channel();
    let thread_name = format!("mqtt-{}", profile.display_name());

    let join = std::thread::Builder::new()
        .name(thread_name)
        .spawn(move || {
            let emitter = Emitter { tx: event_tx, ctx };

            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    emitter.state(ConnectionState::Error(
                        t!("mqtt.runtime_failed", error = err).to_string(),
                    ));
                    return;
                }
            };

            runtime.block_on(run(profile, command_rx, emitter));
        })
        .expect("Could not start the MQTT worker thread");

    ConnectionHandle {
        commands,
        events,
        join: Some(join),
    }
}

#[derive(Clone)]
struct Emitter {
    tx: std::sync::mpsc::Sender<WorkerEvent>,
    ctx: egui::Context,
}

impl Emitter {
    fn send(&self, event: WorkerEvent) {
        if self.tx.send(event).is_err() {
            log::debug!("MQTT event dropped: UI channel closed");
        }
        self.ctx.request_repaint();
    }

    fn state(&self, state: ConnectionState) {
        self.send(WorkerEvent::State(state));
    }

    fn log(&self, message: impl Into<String>) {
        self.send(WorkerEvent::Log(message.into()));
    }
}

async fn run(
    profile: ConnectionProfile,
    mut commands: tokio::sync::mpsc::UnboundedReceiver<Command>,
    emitter: Emitter,
) {
    emitter.state(ConnectionState::Connecting);

    let client_id = if profile.client_id.trim().is_empty() {
        crate::config::new_client_id()
    } else {
        profile.client_id.trim().to_owned()
    };
    let host = profile.host.trim();
    if host.is_empty() {
        emitter.state(ConnectionState::Error(t!("mqtt.no_host").to_string()));
        return;
    }

    let mut options = if profile.websocket {
        MqttOptions::new(client_id, super::websocket::url_for(&profile), profile.port)
    } else {
        MqttOptions::new(client_id, host, profile.port)
    };
    options
        .set_keep_alive(Duration::from_secs(profile.keep_alive_secs.clamp(5, 3_600)))
        .set_clean_session(profile.clean_session)
        .set_max_packet_size(MAX_PACKET_SIZE, MAX_PACKET_SIZE);

    if !profile.username.is_empty() {
        options.set_credentials(profile.username.clone(), profile.password.clone());
    }
    let transport = if profile.websocket {
        super::websocket::transport_for(&profile)
    } else if profile.tls {
        super::tls::transport_for(&profile)
    } else {
        Ok(Transport::Tcp)
    };
    match transport {
        Ok(transport) => {
            options.set_transport(transport);
        }
        Err(err) => {
            emitter.state(ConnectionState::Error(
                t!(
                    "mqtt.ca_certificate_failed",
                    path = profile.ca_cert_path.trim(),
                    error = err
                )
                .to_string(),
            ));
            return;
        }
    }

    let (client, mut eventloop) = AsyncClient::new(options, REQUEST_CAPACITY);
    let subscribe_topic = profile.subscribe_topic.trim().to_owned();
    let subscribe_qos = qos_from_u8(profile.subscribe_qos);
    let mut pending_subscribe: Option<String> = None;
    let mut last_error_log: Option<Instant> = None;
    let mut last_state = ConnectionState::Connecting;

    loop {
        tokio::select! {
            command = commands.recv() => {
                match command {
                    Some(Command::Publish { topic, payload, qos, retain }) => {
                        let size = payload.len();
                        match client.publish(topic.clone(), qos_from_u8(qos), retain, payload).await {
                            Ok(()) => emitter.log(t!(
                                "mqtt.publish_ok",
                                topic = topic,
                                size = size
                            )),
                            Err(err) => emitter.log(
                                t!("mqtt.publish_failed", error = err).to_string(),
                            ),
                        }
                    }
                    Some(Command::Disconnect) | Some(Command::Shutdown) | None => {
                        let _ = client.disconnect().await;
                        break;
                    }
                }
            }
            polled = eventloop.poll() => {
                match polled {
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        if last_state != ConnectionState::Connected {
                            emitter.state(ConnectionState::Connected);
                            last_state = ConnectionState::Connected;
                            emitter.log(
                                t!("mqtt.connected", endpoint = profile.endpoint())
                                    .to_string(),
                            );
                        }
                        if !subscribe_topic.is_empty() {
                            match client.subscribe(subscribe_topic.clone(), subscribe_qos).await {
                                Ok(()) => {
                                    pending_subscribe = Some(subscribe_topic.clone());
                                    emitter.log(
                                        t!(
                                            "mqtt.subscribe_requested",
                                            topic = subscribe_topic
                                        )
                                        .to_string(),
                                    );
                                }
                                Err(err) => emitter.send(WorkerEvent::SubscribeFailed(
                                    t!("mqtt.subscribe_failed", error = err).to_string(),
                                )),
                            }
                        }
                    }
                    Ok(Event::Incoming(Packet::Publish(publish))) => {
                        let message = MessageRecord::new(
                            publish.topic,
                            publish.payload.to_vec(),
                            publish.qos as u8,
                            publish.retain,
                        );
                        emitter.send(WorkerEvent::Message(message));
                    }
                    Ok(Event::Incoming(Packet::SubAck(suback))) => {
                        let topic = pending_subscribe
                            .take()
                            .unwrap_or_else(|| subscribe_topic.clone());
                        if subscription_accepted(&suback.return_codes) {
                            emitter.log(
                                t!("mqtt.subscribe_active", topic = topic).to_string(),
                            );
                            emitter.send(WorkerEvent::Subscribed(topic));
                        } else {
                            emitter.send(WorkerEvent::SubscribeFailed(
                                t!("mqtt.subscribe_rejected", topic = topic).to_string(),
                            ));
                        }
                    }
                    Ok(Event::Incoming(Packet::Disconnect)) => {
                        emitter.state(ConnectionState::Disconnected);
                        break;
                    }
                    Ok(_) => {}
                    Err(err) => {
                        let now = Instant::now();
                        if last_error_log
                            .is_none_or(|last| now.duration_since(last) >= ERROR_LOG_INTERVAL)
                        {
                            emitter.log(
                                t!("mqtt.connection_error", error = err).to_string(),
                            );
                            last_error_log = Some(now);
                        }
                        if last_state != ConnectionState::Reconnecting {
                            emitter.state(ConnectionState::Reconnecting);
                            last_state = ConnectionState::Reconnecting;
                        }
                        tokio::time::sleep(RECONNECT_DELAY).await;
                    }
                }
            }
        }
    }

    emitter.state(ConnectionState::Disconnected);
}

fn qos_from_u8(qos: u8) -> QoS {
    match qos {
        1 => QoS::AtLeastOnce,
        2 => QoS::ExactlyOnce,
        _ => QoS::AtMostOnce,
    }
}

fn subscription_accepted(codes: &[SubscribeReasonCode]) -> bool {
    !codes.is_empty()
        && codes
            .iter()
            .all(|code| matches!(code, SubscribeReasonCode::Success(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suback_requires_every_filter_to_succeed() {
        assert!(subscription_accepted(&[SubscribeReasonCode::Success(
            QoS::AtLeastOnce
        )]));
        assert!(subscription_accepted(&[
            SubscribeReasonCode::Success(QoS::AtMostOnce),
            SubscribeReasonCode::Success(QoS::ExactlyOnce),
        ]));
        assert!(!subscription_accepted(&[]));
        assert!(!subscription_accepted(&[SubscribeReasonCode::Failure]));
        assert!(!subscription_accepted(&[
            SubscribeReasonCode::Success(QoS::AtMostOnce),
            SubscribeReasonCode::Failure,
        ]));
    }
}
