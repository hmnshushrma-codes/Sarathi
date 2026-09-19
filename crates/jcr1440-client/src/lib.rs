//! Async client for the JCR1440 OBD-II/GPS telematics device.
//!
//! Reimplements the MD5 challenge-response auth flow from the Python poller
//! (scripts/jcr1440-poller.py) and provides a polling loop that yields parsed
//! `TelemetryFrame` structs.
//!
//! # Auth protocol (from real device)
//!
//! 1. GET /mark_lang.w.xml → extract `<rand>` nonce
//! 2. GET /login.htm → extract csrf_token2 hidden input value
//! 3. Compute `MD5(rand + password)` as hex
//! 4. POST /wxml/post_login.xml with Name, password (hash), rand, CSRF header
//! 5. Response sets SessionID cookie; `<login_check>3</login_check>` = success
//!
//! WiFi key is plaintext, but login password is MD5(rand + cleartext).

mod parse;

use std::net::IpAddr;
use std::time::Duration;

use md5::{Digest, Md5};
use regex::Regex;
use reqwest::{Client, StatusCode};
use thiserror::Error;
use tokio::sync::watch;
use tokio::time;
use tracing::{debug, error, info, warn};

pub use parse::{GpsData, ObdData, TelemetryFrame};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum Jcr1440Error {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("XML parse error: {0}")]
    Xml(String),

    #[error("Auth failed: login_check={0}")]
    AuthFailed(String),

    #[error("Could not extract {field} from response")]
    ParseField { field: &'static str },

    #[error("Device unreachable")]
    Unreachable,
}

pub type Result<T> = std::result::Result<T, Jcr1440Error>;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DeviceConfig {
    pub base_url: String,
    pub host_header: String,
    pub interface: String,
    pub username: String,
    pub password: String,
    pub poll_interval: Duration,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            base_url: "http://192.168.1.1".into(),
            host_header: "jiocarfi.local.html".into(),
            interface: "enxfcde56ff0106".into(),
            username: "administrator".into(),
            password: "administrator".into(),
            poll_interval: Duration::from_millis(300),
        }
    }
}

// ---------------------------------------------------------------------------
// Interface resolution
// ---------------------------------------------------------------------------

/// Resolve a network interface name (e.g. "enxfcde56ff0106") to its IPv4 address
/// by parsing the output of `ip -4 -o addr show <iface>`.
fn resolve_interface_addr(iface: &str) -> Option<IpAddr> {
    // Try /usr/sbin/ip first (Ubuntu/Debian), fall back to bare "ip"
    let ip_bin = if std::path::Path::new("/usr/sbin/ip").exists() {
        "/usr/sbin/ip"
    } else {
        "ip"
    };
    let output = std::process::Command::new(ip_bin)
        .args(["-4", "-o", "addr", "show", iface])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    // Format: "3: enxfcde56ff0106    inet 192.168.1.100/24 brd ..."
    // Find the token after "inet" and strip the CIDR prefix length.
    let mut tokens = text.split_whitespace();
    while let Some(tok) = tokens.next() {
        if tok == "inet" {
            if let Some(cidr) = tokens.next() {
                let ip_str = cidr.split('/').next()?;
                return ip_str.parse::<IpAddr>().ok();
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Authenticated session with the JCR1440 device.
pub struct Jcr1440Client {
    config: DeviceConfig,
    http: Client,
    csrf_re: Regex,
}

impl Jcr1440Client {
    pub fn new(config: DeviceConfig) -> Result<Self> {
        // reqwest's cookie store handles SessionID automatically.
        // Bind to the RNDIS interface IP so traffic goes to the dongle,
        // not the home router on the same 192.168.1.0/24 subnet.
        let mut builder = Client::builder()
            .cookie_store(true)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10));

        if let Some(addr) = resolve_interface_addr(&config.interface) {
            info!("Binding HTTP client to {} ({})", config.interface, addr);
            builder = builder.local_address(addr);
        } else {
            warn!(
                "Could not resolve IP for interface '{}'; requests may route to wrong device",
                config.interface
            );
        }

        let http = builder.build()?;

        let csrf_re =
            Regex::new(r#"id="csrf_token2"[^>]*value="([^"]+)""#).expect("static regex");

        Ok(Self {
            config,
            http,
            csrf_re,
        })
    }

    // -- Low-level HTTP helpers -----------------------------------------------

    async fn get(&self, path: &str) -> Result<String> {
        let url = format!("{}{}", self.config.base_url, path);
        let resp = self
            .http
            .get(&url)
            .header("Host", &self.config.host_header)
            .send()
            .await?;

        if resp.status() != StatusCode::OK {
            return Err(Jcr1440Error::Unreachable);
        }
        Ok(resp.text().await?)
    }

    async fn post_form(&self, path: &str, body: &str, csrf: &str) -> Result<String> {
        let url = format!("{}{}", self.config.base_url, path);
        let resp = self
            .http
            .post(&url)
            .header("Host", &self.config.host_header)
            .header("__RequestVerificationToken", csrf)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body.to_owned())
            .send()
            .await?;

        Ok(resp.text().await?)
    }

    // -- Auth flow ------------------------------------------------------------

    /// Perform the full MD5 challenge-response login.
    ///
    /// Mirrors the Python flow:
    /// ```python
    /// rand = GET /mark_lang.w.xml → <rand>
    /// csrf = GET /login.htm → csrf_token2 hidden value
    /// hash = md5(rand + password).hexdigest()
    /// POST /wxml/post_login.xml { Name, password=hash, rand } + CSRF header
    /// ```
    pub async fn login(&self) -> Result<()> {
        // Step 1: get rand nonce
        let lang_body = self.get("/mark_lang.w.xml").await?;
        let rand = parse::extract_xml_text(&lang_body, "rand")
            .ok_or(Jcr1440Error::ParseField { field: "rand" })?;

        debug!(rand = %rand, "Got auth nonce");

        // Step 2: get CSRF token from login.htm
        let login_page = self.get("/login.htm").await?;
        let csrf = self
            .csrf_re
            .captures(&login_page)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_owned())
            .ok_or(Jcr1440Error::ParseField {
                field: "csrf_token2",
            })?;

        debug!("Got CSRF token");

        // Step 3: MD5(rand + password)
        let mut hasher = Md5::new();
        hasher.update(format!("{}{}", rand, self.config.password));
        let pass_hash = format!("{:x}", hasher.finalize());

        // Step 4: POST login
        let body = format!(
            "Name={}&password={}&rand={}",
            self.config.username, pass_hash, rand
        );
        let resp = self.post_form("/wxml/post_login.xml", &body, &csrf).await?;

        let check = parse::extract_xml_text(&resp, "login_check").unwrap_or_default();
        if check != "3" {
            return Err(Jcr1440Error::AuthFailed(check));
        }

        info!("Authenticated with JCR1440");
        Ok(())
    }

    /// Fetch and parse a single telemetry frame from st_gps.w.xml.
    pub async fn poll_telemetry(&self) -> Result<TelemetryFrame> {
        let body = self.get("/st_gps.w.xml").await?;

        // Detect login redirect (session expired)
        if body.contains("login.htm") && body.len() < 300 {
            warn!("Session expired, re-authenticating");
            self.login().await?;
            let body = self.get("/st_gps.w.xml").await?;
            return parse::parse_telemetry_frame(&body);
        }

        parse::parse_telemetry_frame(&body)
    }

    /// Clean logout.
    pub async fn logout(&self) {
        let _ = self.get("/wxml/login_exit.xml").await;
    }
}

// ---------------------------------------------------------------------------
// Polling loop — yields frames on a watch channel
// ---------------------------------------------------------------------------

/// Current state of the device connection, sent over the watch channel.
#[derive(Debug, Clone)]
pub enum DeviceState {
    /// No connection attempt yet.
    Disconnected,
    /// Trying to connect/authenticate.
    Connecting,
    /// Successfully polling data.
    Live(TelemetryFrame),
    /// Device unreachable or erroring; includes consecutive-failure count.
    Error { message: String, failures: u32 },
}

/// Spawn a background polling task that pushes `DeviceState` updates.
///
/// Returns a watch::Receiver the rendering thread can read without blocking.
/// The task handles login, re-auth on session expiry, and exponential backoff
/// on transient failures.
pub fn spawn_poller(config: DeviceConfig) -> watch::Receiver<DeviceState> {
    let (tx, rx) = watch::channel(DeviceState::Disconnected);

    tokio::spawn(async move {
        let poll_interval = config.poll_interval;
        let client = match Jcr1440Client::new(config) {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to create client: {}", e);
                let _ = tx.send(DeviceState::Error {
                    message: e.to_string(),
                    failures: 1,
                });
                return;
            }
        };

        let mut consecutive_failures: u32 = 0;

        // Initial login with retries
        loop {
            let _ = tx.send(DeviceState::Connecting);
            match client.login().await {
                Ok(()) => break,
                Err(e) => {
                    consecutive_failures += 1;
                    let delay = backoff_delay(consecutive_failures);
                    warn!("Login failed (attempt {}): {}. Retrying in {:?}", consecutive_failures, e, delay);
                    let _ = tx.send(DeviceState::Error {
                        message: e.to_string(),
                        failures: consecutive_failures,
                    });
                    time::sleep(delay).await;
                }
            }
        }
        consecutive_failures = 0;

        // Main poll loop
        let mut interval = time::interval(poll_interval);
        loop {
            interval.tick().await;

            match client.poll_telemetry().await {
                Ok(frame) => {
                    consecutive_failures = 0;
                    let _ = tx.send(DeviceState::Live(frame));
                }
                Err(e) => {
                    consecutive_failures += 1;
                    let delay = backoff_delay(consecutive_failures);
                    warn!(
                        "Poll failed ({}x): {}. Backoff {:?}",
                        consecutive_failures, e, delay
                    );
                    let _ = tx.send(DeviceState::Error {
                        message: e.to_string(),
                        failures: consecutive_failures,
                    });

                    // On repeated failures, try re-login
                    if consecutive_failures >= 3 {
                        info!("Attempting re-login after {} failures", consecutive_failures);
                        if let Err(e) = client.login().await {
                            error!("Re-login failed: {}", e);
                        }
                    }

                    time::sleep(delay).await;
                }
            }
        }
    });

    rx
}

fn backoff_delay(failures: u32) -> Duration {
    let secs = (1u64 << failures.min(5)).min(30);
    Duration::from_secs(secs)
}

// ---------------------------------------------------------------------------
// Mock data source for development without a real device
// ---------------------------------------------------------------------------

/// Spawn a fake poller that generates synthetic telemetry data,
/// cycling RPM 0→8000→0 over ~4 seconds for smooth gauge testing.
pub fn spawn_mock_poller() -> watch::Receiver<DeviceState> {
    let (tx, rx) = watch::channel(DeviceState::Disconnected);

    tokio::spawn(async move {
        let _ = tx.send(DeviceState::Connecting);
        time::sleep(Duration::from_millis(500)).await;

        let mut tick: u64 = 0;
        let mut interval = time::interval(Duration::from_millis(50));

        loop {
            interval.tick().await;
            tick += 1;

            // RPM: sinusoidal sweep 0 → 8000 over ~4s (80 ticks at 50ms)
            let phase = (tick as f64 * std::f64::consts::PI * 2.0) / 80.0;
            let rpm = ((phase.sin() + 1.0) / 2.0 * 8000.0) as f32;

            // Speed ramps with RPM (simulating gear ratio)
            let speed = (rpm / 8000.0 * 220.0) as f32;

            let frame = TelemetryFrame {
                gps: GpsData {
                    speed: speed,
                    longitude: 72.8776 + (tick as f64 * 0.00001) % 0.01,
                    latitude: 19.0760 + (tick as f64 * 0.000005) % 0.005,
                    altitude: 215.0,
                    heading: ((tick as f32 * 0.5) % 360.0),
                    satellites: 12,
                    hdop: 1.2,
                    accuracy: 8.0,
                    fix_valid: true,
                    timestamp_ms: 0,
                },
                obd: ObdData {
                    vin: Some("TESTVIN1234567890".into()),
                    engine_rpm: Some(rpm),
                    vehicle_speed: Some(speed),
                    coolant_temp: Some(85.0 + (phase.sin() as f32 * 10.0)),
                    intake_air_temp: Some(32.0 + (phase.cos() as f32 * 5.0)),
                    battery_voltage: Some(13.8 + (phase.sin() as f32 * 0.4)),
                    throttle_position: Some((rpm / 8000.0 * 100.0) as f32),
                    maf: Some(rpm / 8000.0 * 150.0),
                    fuel_level: Some(72.0),
                    mil_status: Some(false),
                    dtc_count: Some(0),
                    dtc_codes: None,
                    manifold_pressure: Some(30.0 + rpm / 8000.0 * 70.0),
                    oil_temp: Some(95.0 + (phase.sin() as f32 * 8.0)),
                    ambient_air_temp: Some(28.0),
                    fuel_rate: Some(rpm / 8000.0 * 25.0),
                },
                device_connected: true,
            };

            let _ = tx.send(DeviceState::Live(frame));
        }
    });

    rx
}
