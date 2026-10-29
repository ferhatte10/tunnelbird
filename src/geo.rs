// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright 2026 Ferhat Saidoun

//! Public IP address and approximate location, as the internet sees them.
//!
//! Queried from <https://ifconfig.co> (HTTPS, no account) once a tunnel that
//! carries all traffic is connected, so the answer describes the VPN exit.
//! The request goes through the tunnel like any other traffic.

use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

use gtk::glib;
use gtk::glib::translate::IntoGlib;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use soup::prelude::*;

const SERVICE_URL: &str = "https://ifconfig.co/json";
/// Country list shipped by the `iso-codes` package, with translations under the
/// `iso_3166-1` gettext domain.
const ISO_CODES_JSON: &str = "/usr/share/iso-codes/json/iso_3166-1.json";
const ISO_CODES_DOMAIN: &str = "iso_3166-1";

#[derive(Debug, Clone, Deserialize)]
pub struct Location {
    pub ip: String,
    pub country: Option<String>,
    /// ISO 3166-1 alpha-2 code, e.g. `FR`.
    pub country_iso: Option<String>,
    pub city: Option<String>,
    /// Network operator owning the IP (hosting provider, ISP…).
    pub asn_org: Option<String>,
}

#[derive(Debug)]
pub enum HttpError {
    InvalidUrl(glib::BoolError),
    Request(glib::Error),
    Status(u32),
    Parse(serde_json::Error),
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HttpError::InvalidUrl(err) => write!(f, "{err}"),
            HttpError::Request(err) => write!(f, "{err}"),
            HttpError::Status(code) => write!(f, "HTTP {code}"),
            HttpError::Parse(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for HttpError {}

/// One HTTP session for the whole app (connection reuse, shared timeout).
pub fn new_session() -> soup::Session {
    soup::Session::builder()
        .timeout(10)
        .user_agent(concat!("Tunnelbird/", env!("CARGO_PKG_VERSION")))
        .build()
}

pub async fn lookup(session: &soup::Session) -> Result<Location, HttpError> {
    get_json(session, SERVICE_URL).await
}

/// GETs `url` and parses its JSON answer (also used by the update check).
pub async fn get_json<T: DeserializeOwned>(
    session: &soup::Session,
    url: &str,
) -> Result<T, HttpError> {
    let message = soup::Message::new("GET", url).map_err(HttpError::InvalidUrl)?;
    if let Some(headers) = message.request_headers() {
        headers.append("Accept", "application/json");
    }
    let body = session
        .send_and_read_future(&message, glib::Priority::DEFAULT)
        .await
        .map_err(HttpError::Request)?;
    let status = message.status().into_glib() as u32;
    if !(200..300).contains(&status) {
        return Err(HttpError::Status(status));
    }
    serde_json::from_slice(&body).map_err(HttpError::Parse)
}

/// Flag emoji and "City, Country" (country name in the user's language) for display.
pub fn describe(location: &Location) -> (String, String) {
    let country = location
        .country_iso
        .as_deref()
        .and_then(country_name)
        .or_else(|| location.country.clone());
    let place = [location.city.as_deref(), country.as_deref()]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    let flag = location
        .country_iso
        .as_deref()
        .and_then(flag)
        .unwrap_or_else(|| "🌐".to_owned());
    (flag, place)
}

/// Flag emoji for an ISO country code (`FR` → 🇫🇷), using regional indicator symbols.
pub fn flag(country_iso: &str) -> Option<String> {
    let code = country_iso.trim();
    if code.len() != 2 || !code.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    code.to_ascii_uppercase()
        .chars()
        .map(|c| char::from_u32(0x1F1E6 + (c as u32 - 'A' as u32)))
        .collect()
}

#[derive(Deserialize)]
struct IsoCountries {
    #[serde(rename = "3166-1")]
    countries: Vec<IsoCountry>,
}

#[derive(Deserialize)]
struct IsoCountry {
    alpha_2: String,
    name: String,
    common_name: Option<String>,
}

/// Country name in the user's language (`DE` → “Allemagne” in French), from the
/// system `iso-codes` database. Returns `None` when the code is unknown or the
/// database is missing, so the caller can fall back to the service's English name.
pub fn country_name(country_iso: &str) -> Option<String> {
    static NAMES: OnceLock<HashMap<String, String>> = OnceLock::new();
    let names = NAMES.get_or_init(|| {
        let parsed = std::fs::read(ISO_CODES_JSON)
            .map_err(|err| err.to_string())
            .and_then(|bytes| {
                serde_json::from_slice::<IsoCountries>(&bytes).map_err(|err| err.to_string())
            });
        match parsed {
            Ok(list) => list
                .countries
                .into_iter()
                .map(|c| (c.alpha_2, c.common_name.unwrap_or(c.name)))
                .collect(),
            Err(err) => {
                glib::g_debug!(crate::LOG_DOMAIN, "No iso-codes country list: {err}");
                HashMap::new()
            }
        }
    });
    let english = names.get(&country_iso.trim().to_ascii_uppercase())?;
    Some(gettextrs::dgettext(ISO_CODES_DOMAIN, english.as_str()))
}
