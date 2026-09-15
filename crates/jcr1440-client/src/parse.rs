//! XML parsing for JCR1440 telemetry responses.
//!
//! Field names and formats are taken directly from the real device response
//! (st_gps.w.xml dump in diagnostics/jcr1440/obd-fields-20260915.json).

use crate::Jcr1440Error;
use serde::{Deserialize, Serialize};

/// One complete telemetry snapshot from the device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryFrame {
    pub gps: GpsData,
    pub obd: ObdData,
    pub device_connected: bool,
}

/// GPS fields parsed from the `gps_data` semicolon-delimited string.
///
/// Format: `fix,speed;fix,lon;fix,lat;fix,alt;fix,heading;fix,sats;hdop;vdop;pdop;accuracy;fix,timestamp_ms;fix,mode`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GpsData {
    pub speed: f32,        // km/h (from GPS, not OBD)
    pub longitude: f64,    // degrees
    pub latitude: f64,     // degrees
    pub altitude: f32,     // meters
    pub heading: f32,      // degrees
    pub satellites: u8,
    pub hdop: f32,
    pub accuracy: f32,     // meters
    pub fix_valid: bool,
    pub timestamp_ms: u64,
}

/// OBD-II fields — all Optional because they return "-" when the device
/// isn't connected to a vehicle's OBD-II port.
///
/// XML tag names from the real st_gps.w.xml response:
/// vin, am_air_temp, celv, dtwma, ddtc, ect, efr, eot, erpm, ert, fl,
/// iat, pmsmc, tses, tsdtcc, tert, toff, vspeed, mils, dtc, maf, capab,
/// atp, imap, dr
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ObdData {
    pub vin: Option<String>,
    /// erpm — Engine RPM
    pub engine_rpm: Option<f32>,
    /// vspeed — Vehicle Speed (km/h, from ECU, more accurate than GPS speed)
    pub vehicle_speed: Option<f32>,
    /// ect — Engine Coolant Temperature (°C)
    pub coolant_temp: Option<f32>,
    /// iat — Intake Air Temperature (°C)
    pub intake_air_temp: Option<f32>,
    /// celv — Battery Voltage (V)
    pub battery_voltage: Option<f32>,
    /// atp — Absolute Throttle Position (%)
    pub throttle_position: Option<f32>,
    /// maf — Mass Air Flow (g/s)
    pub maf: Option<f32>,
    /// fl — Fuel Level (%)
    pub fuel_level: Option<f32>,
    /// mils — Malfunction Indicator Lamp status
    pub mil_status: Option<bool>,
    /// Count of active DTCs (derived from dtc field)
    pub dtc_count: Option<u32>,
    /// dtc — Raw DTC code string
    pub dtc_codes: Option<String>,
    /// imap — Intake Manifold Absolute Pressure (kPa)
    pub manifold_pressure: Option<f32>,
    /// eot — Engine Oil Temperature (°C)
    pub oil_temp: Option<f32>,
    /// am_air_temp — Ambient Air Temperature (°C)
    pub ambient_air_temp: Option<f32>,
    /// efr — Engine Fuel Rate (L/h)
    pub fuel_rate: Option<f32>,
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

/// Extract text content of a simple XML element by tag name.
/// This is deliberately simple — the JCR1440 returns flat XML without
/// namespaces or deep nesting, so a regex-like scan works fine.
pub fn extract_xml_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    let text = xml[start..end].trim().to_string();
    if text.is_empty() || text == "-" {
        None
    } else {
        Some(text)
    }
}

/// Parse a "fix,value" pair from the gps_data string. Returns the value part.
fn parse_fix_value(s: &str) -> Option<&str> {
    let (fix, val) = s.split_once(',')?;
    if fix == "1" {
        Some(val)
    } else {
        // fix=0 means invalid reading
        None
    }
}

/// Parse the gps_data semicolon-delimited string.
fn parse_gps_data(raw: &str) -> GpsData {
    let parts: Vec<&str> = raw.split(';').collect();
    let mut gps = GpsData::default();

    if parts.len() < 12 {
        return gps;
    }

    // Index 0: fix,speed
    if let Some(v) = parse_fix_value(parts[0]) {
        gps.speed = v.parse().unwrap_or(0.0);
        gps.fix_valid = true;
    }
    // Index 1: fix,longitude
    if let Some(v) = parse_fix_value(parts[1]) {
        gps.longitude = v.parse().unwrap_or(0.0);
    }
    // Index 2: fix,latitude
    if let Some(v) = parse_fix_value(parts[2]) {
        gps.latitude = v.parse().unwrap_or(0.0);
    }
    // Index 3: fix,altitude
    if let Some(v) = parse_fix_value(parts[3]) {
        gps.altitude = v.parse().unwrap_or(0.0);
    }
    // Index 4: fix,heading
    if let Some(v) = parse_fix_value(parts[4]) {
        gps.heading = v.parse().unwrap_or(0.0);
    }
    // Index 5: fix,satellites
    if let Some(v) = parse_fix_value(parts[5]) {
        gps.satellites = v.parse().unwrap_or(0);
    }
    // Index 6: hdop (no fix prefix)
    gps.hdop = parts[6].parse().unwrap_or(99.0);
    // Index 9: accuracy
    gps.accuracy = parts[9].parse().unwrap_or(99.0);
    // Index 10: fix,timestamp_ms
    if let Some(v) = parse_fix_value(parts[10]) {
        gps.timestamp_ms = v.parse().unwrap_or(0);
    }

    gps
}

fn parse_float(xml: &str, tag: &str) -> Option<f32> {
    extract_xml_text(xml, tag)?.parse().ok()
}

fn parse_bool_field(xml: &str, tag: &str) -> Option<bool> {
    let val = extract_xml_text(xml, tag)?;
    match val.as_str() {
        "0" => Some(false),
        "1" => Some(true),
        _ => None,
    }
}

/// Parse a complete telemetry frame from the raw st_gps.w.xml body.
pub fn parse_telemetry_frame(xml: &str) -> crate::Result<TelemetryFrame> {
    let gps_raw = extract_xml_text(xml, "gps_data")
        .ok_or(Jcr1440Error::ParseField { field: "gps_data" })?;

    let gps = parse_gps_data(&gps_raw);

    // Parse OBD fields — each returns None when the tag value is "-"
    let dtc_raw = extract_xml_text(xml, "dtc");
    let dtc_count = dtc_raw.as_ref().map(|s| {
        if s.is_empty() {
            0u32
        } else {
            // DTCs are typically comma-separated codes
            s.split(',').filter(|c| !c.trim().is_empty()).count() as u32
        }
    });

    let obd = ObdData {
        vin: extract_xml_text(xml, "vin"),
        engine_rpm: parse_float(xml, "erpm"),
        vehicle_speed: parse_float(xml, "vspeed"),
        coolant_temp: parse_float(xml, "ect"),
        intake_air_temp: parse_float(xml, "iat"),
        battery_voltage: parse_float(xml, "celv"),
        throttle_position: parse_float(xml, "atp"),
        maf: parse_float(xml, "maf"),
        fuel_level: parse_float(xml, "fl"),
        mil_status: parse_bool_field(xml, "mils"),
        dtc_count,
        dtc_codes: dtc_raw,
        manifold_pressure: parse_float(xml, "imap"),
        oil_temp: parse_float(xml, "eot"),
        ambient_air_temp: parse_float(xml, "am_air_temp"),
        fuel_rate: parse_float(xml, "efr"),
    };

    Ok(TelemetryFrame {
        gps,
        obd,
        device_connected: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real device response (captured 2026-09-15, device not in vehicle)
    const REAL_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<title>
	<gps_data>1,0;1,72.877655;1,19.075983;1,0.000000;0,0.000000;1,12;1.400000;1.000000;1.700000;9.000000;1,1700000000000;1,2</gps_data>
	<vin>-</vin>
	<am_air_temp>-</am_air_temp>
	<celv>-</celv>
	<erpm>-</erpm>
	<ect>-</ect>
	<vspeed>-</vspeed>
	<mils>-</mils>
	<dtc>-</dtc>
	<maf>-</maf>
	<atp>-</atp>
	<imap>-</imap>
	<fl>-</fl>
	<iat>-</iat>
	<eot>-</eot>
	<efr>-</efr>
	<celv>-</celv>
	<dr>-</dr>
</title>"#;

    #[test]
    fn parse_real_offline_response() {
        let frame = parse_telemetry_frame(REAL_XML).unwrap();

        // GPS should be parsed from the gps_data string
        assert!(frame.gps.fix_valid);
        assert!((frame.gps.latitude - 19.075983).abs() < 0.0001);
        assert!((frame.gps.longitude - 72.877655).abs() < 0.0001);
        assert_eq!(frame.gps.satellites, 12);
        assert!((frame.gps.hdop - 1.4).abs() < 0.01);

        // OBD should all be None (device not in vehicle)
        assert!(frame.obd.engine_rpm.is_none());
        assert!(frame.obd.vehicle_speed.is_none());
        assert!(frame.obd.coolant_temp.is_none());
        assert!(frame.obd.battery_voltage.is_none());
        assert!(frame.obd.dtc_codes.is_none());
    }

    // Synthetic response simulating data from a connected vehicle
    const VEHICLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<title>
	<gps_data>1,65;1,77.1;1,28.4;1,215.0;1,90.0;1,12;1.2;0.9;1.5;8.0;1,1789460000000;1,3</gps_data>
	<vin>MALA851CLHM123456</vin>
	<erpm>3200</erpm>
	<vspeed>65</vspeed>
	<ect>88</ect>
	<iat>35</iat>
	<celv>13.8</celv>
	<atp>42</atp>
	<maf>24.5</maf>
	<fl>72</fl>
	<mils>0</mils>
	<dtc>-</dtc>
	<imap>55</imap>
	<eot>96</eot>
	<am_air_temp>28</am_air_temp>
	<efr>8.2</efr>
	<dr>450</dr>
</title>"#;

    #[test]
    fn parse_vehicle_connected_response() {
        let frame = parse_telemetry_frame(VEHICLE_XML).unwrap();

        assert_eq!(frame.obd.engine_rpm, Some(3200.0));
        assert_eq!(frame.obd.vehicle_speed, Some(65.0));
        assert_eq!(frame.obd.coolant_temp, Some(88.0));
        assert_eq!(frame.obd.intake_air_temp, Some(35.0));
        assert_eq!(frame.obd.battery_voltage, Some(13.8));
        assert_eq!(frame.obd.throttle_position, Some(42.0));
        assert_eq!(frame.obd.mil_status, Some(false));
        assert_eq!(frame.obd.dtc_count, None); // dtc is "-"
        assert_eq!(frame.obd.fuel_level, Some(72.0));
        assert_eq!(frame.obd.manifold_pressure, Some(55.0));

        assert_eq!(frame.gps.speed, 65.0);
        assert_eq!(frame.gps.satellites, 12);
    }
}
