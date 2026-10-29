// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! The protocol between a native interface and the tunnel service, on the
//! systems where tunnels need a privileged helper (Windows service, macOS
//! launchd helper). Linux does not use it: NetworkManager is that service.
//!
//! Transport: a local channel only (Windows named pipe, macOS Unix socket),
//! one JSON object per line, UTF-8. The interface sends [`Request`]s, each
//! with an `id`; the service answers each with a [`Response`] carrying the
//! same `id`, and pushes [`Event`]s at any time (state changes, traffic).
//! Everything the service writes is a [`ServiceMessage`].
//!
//! The interface is in C# (Windows) or Swift (macOS): this module is the
//! reference for their types, and `docs/PROTOCOL.md` describes the JSON with
//! examples. Field and variant names are part of the protocol: renaming one
//! is a breaking change and bumps [`VERSION`].

use serde::{Deserialize, Serialize};

use crate::profile::Kind;

/// Protocol version, checked by [`Request::Hello`].
pub const VERSION: u32 = 1;

/// A request from the interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    /// Chosen by the interface; the answer carries the same one.
    pub id: u64,
    #[serde(flatten)]
    pub body: RequestBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "kebab-case")]
pub enum RequestBody {
    /// First message: the service answers with its own version and refuses
    /// other requests until the versions match.
    Hello {
        version: u32,
    },
    /// All tunnels with their state.
    ListTunnels,
    /// Stores a tunnel from a `.conf` or `.ovpn` file's text, as edited by the
    /// user. Certificates must be inline. Answer: [`ResponseBody::Tunnel`].
    Import {
        name: String,
        text: String,
        /// OpenVPN user name and password, never written in the file.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        login: Option<Login>,
    },
    /// Replaces a tunnel's file and name; a connected tunnel keeps its old
    /// settings until it reconnects.
    Update {
        tunnel: String,
        name: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        login: Option<Login>,
    },
    Delete {
        tunnel: String,
    },
    /// Connects a tunnel, disconnecting any other first (one at a time).
    Connect {
        tunnel: String,
    },
    Disconnect {
        tunnel: String,
    },
    /// The tunnel's file, keys included, for the editor and sharing.
    /// Answer: [`ResponseBody::Text`].
    Export {
        tunnel: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Login {
    pub username: String,
    /// Empty: asked at each connection through [`Event::PasswordNeeded`].
    #[serde(default)]
    pub password: String,
}

/// What the service writes: an answer or an event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ServiceMessage {
    Response(Response),
    Event(EventMessage),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// The request's `id`.
    pub id: u64,
    #[serde(flatten)]
    pub body: ResponseBody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "response", rename_all = "kebab-case")]
pub enum ResponseBody {
    Ok,
    Hello {
        version: u32,
    },
    Tunnels {
        tunnels: Vec<Tunnel>,
    },
    Tunnel {
        tunnel: Tunnel,
    },
    Text {
        text: String,
    },
    /// The request failed. The interface shows its own translated text for
    /// `code`, with `message` (English) as the details.
    Error {
        code: ErrorCode,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorCode {
    VersionMismatch,
    /// The file or the login is not valid (message from the parser).
    InvalidProfile,
    UnknownTunnel,
    /// The tunnel could not be created or started (driver, network…).
    ConnectionFailed,
    Internal,
}

/// An event pushed by the service, outside any request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventMessage {
    #[serde(flatten)]
    pub event: Event,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum Event {
    /// A tunnel was added, changed or removed: the full list again.
    TunnelsChanged { tunnels: Vec<Tunnel> },
    /// A tunnel's state changed. `error` explains a failed connection.
    State {
        tunnel: String,
        state: State,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Interface counters of the connected tunnel, once a second.
    Traffic { tunnel: String, counters: Counters },
    /// An OpenVPN server asks for the password; answer with a `connect` after
    /// an `update` carrying it, or cancel with a `disconnect`.
    PasswordNeeded { tunnel: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
}

/// A tunnel as the interface shows it. Keys never travel this way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tunnel {
    /// Stable identifier chosen by the service.
    pub id: String,
    pub name: String,
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// This computer's addresses inside the VPN, once known.
    #[serde(default)]
    pub addresses: Vec<String>,
    /// All internet traffic goes through the tunnel.
    pub full_tunnel: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    pub state: State,
}

/// Cumulative counters since the tunnel connected; the interface computes
/// rates and the server health from them (see [`crate::health`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counters {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
}

impl Request {
    /// Reads one line from the interface.
    pub fn from_line(line: &str) -> serde_json::Result<Self> {
        serde_json::from_str(line)
    }
}

impl ServiceMessage {
    /// One line for the interface, newline included.
    pub fn to_line(&self) -> String {
        // Serializing these types cannot fail: plain data, string keys.
        let mut line = serde_json::to_string(self).unwrap_or_default();
        line.push('\n');
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_read_as_documented() {
        let request = Request::from_line(r#"{"id":7,"request":"connect","tunnel":"a1"}"#)
            .expect("valid request");
        assert_eq!(
            request,
            Request {
                id: 7,
                body: RequestBody::Connect {
                    tunnel: "a1".to_owned()
                }
            }
        );
        let import = Request::from_line(
            r#"{"id":1,"request":"import","name":"Home","text":"[Interface]","login":{"username":"alex"}}"#,
        )
        .expect("valid request");
        assert!(matches!(
            import.body,
            RequestBody::Import { login: Some(Login { ref password, .. }), .. } if password.is_empty()
        ));
        assert!(Request::from_line(r#"{"id":2,"request":"format-disk"}"#).is_err());
    }

    #[test]
    fn service_messages_write_as_documented() {
        let tunnel = Tunnel {
            id: "a1".to_owned(),
            name: "Office".to_owned(),
            kind: Kind::OpenVpn,
            endpoint: Some("vpn.example.com:1194".to_owned()),
            addresses: vec![],
            full_tunnel: true,
            provider: None,
            state: State::Connected,
        };
        let response = ServiceMessage::Response(Response {
            id: 3,
            body: ResponseBody::Tunnel { tunnel },
        });
        assert_eq!(
            response.to_line(),
            "{\"id\":3,\"response\":\"tunnel\",\"tunnel\":{\"id\":\"a1\",\"name\":\"Office\",\"kind\":\"openvpn\",\"endpoint\":\"vpn.example.com:1194\",\"addresses\":[],\"full_tunnel\":true,\"state\":\"connected\"}}\n"
        );
        let event = ServiceMessage::Event(EventMessage {
            event: Event::State {
                tunnel: "a1".to_owned(),
                state: State::Disconnected,
                error: Some("The server does not answer".to_owned()),
            },
        });
        assert_eq!(
            event.to_line(),
            "{\"event\":\"state\",\"tunnel\":\"a1\",\"state\":\"disconnected\",\"error\":\"The server does not answer\"}\n"
        );
    }

    #[test]
    fn messages_round_trip() {
        let messages = [
            ServiceMessage::Response(Response {
                id: 1,
                body: ResponseBody::Error {
                    code: ErrorCode::InvalidProfile,
                    message: "No server".to_owned(),
                },
            }),
            ServiceMessage::Event(EventMessage {
                event: Event::Traffic {
                    tunnel: "a1".to_owned(),
                    counters: Counters {
                        rx_bytes: 10,
                        tx_bytes: 20,
                        rx_packets: 1,
                        tx_packets: 2,
                    },
                },
            }),
        ];
        for message in messages {
            let back: ServiceMessage =
                serde_json::from_str(message.to_line().trim()).expect("readable");
            assert_eq!(back, message);
        }
    }
}
