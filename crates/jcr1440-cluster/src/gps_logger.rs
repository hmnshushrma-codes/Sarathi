//! GPS track logger — records coordinates during drives.
//!
//! Logs GPS positions to a GPX file whenever a valid fix is available.
//! Each drive session creates a new track segment. Files are saved to
//! /var/log/ninodash/tracks/ with timestamps.
//!
//! The track can later be visualized on a map.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use jcr1440_client::GpsData;

const TRACK_DIR: &str = "/var/log/ninodash/tracks";
const MIN_DISTANCE_M: f64 = 5.0; // minimum meters between logged points
const LOG_INTERVAL_SECS: f64 = 2.0; // minimum seconds between points

pub struct GpsLogger {
    file: Option<File>,
    file_path: Option<PathBuf>,
    last_lat: f64,
    last_lon: f64,
    last_log_time: f64,
    point_count: u64,
    started: bool,
}

impl GpsLogger {
    pub fn new() -> Self {
        // Ensure track directory exists
        let _ = fs::create_dir_all(TRACK_DIR);

        Self {
            file: None,
            file_path: None,
            last_lat: 0.0,
            last_lon: 0.0,
            last_log_time: 0.0,
            point_count: 0,
            started: false,
        }
    }

    /// Start a new track session.
    fn start_session(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let secs = now.as_secs();

        // IST timestamp for filename
        let ist_secs = secs + 5 * 3600 + 30 * 60;
        let days = ist_secs / 86400;
        let day_secs = ist_secs % 86400;
        let hours = day_secs / 3600;
        let mins = (day_secs % 3600) / 60;
        let s = day_secs % 60;

        // Simple date calculation (good enough for filenames)
        let filename = format!("track_{:04}_{:02}_{:02}.gpx",
            hours, mins, s);
        let path = PathBuf::from(TRACK_DIR).join(&filename);

        if let Ok(mut f) = File::create(&path) {
            let header = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="NinoDash"
  xmlns="http://www.topografix.com/GPX/1/1">
  <metadata>
    <name>NinoDash Drive Track</name>
  </metadata>
  <trk>
    <name>Drive</name>
    <trkseg>
"#;
            let _ = f.write_all(header.as_bytes());
            self.file = Some(f);
            self.file_path = Some(path);
            self.started = true;
            self.point_count = 0;
        }
    }

    /// Log a GPS point if it's far enough from the last one.
    pub fn log_point(&mut self, gps: &GpsData) {
        // Only log when we have a valid fix with real coordinates
        if !gps.fix_valid || gps.satellites == 0 {
            return;
        }
        if gps.latitude == 0.0 && gps.longitude == 0.0 {
            return;
        }

        // Start session on first valid point
        if !self.started {
            self.start_session();
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        // Check minimum time interval
        if now - self.last_log_time < LOG_INTERVAL_SECS && self.point_count > 0 {
            return;
        }

        // Check minimum distance (approximate)
        if self.point_count > 0 {
            let dist = haversine_m(self.last_lat, self.last_lon,
                gps.latitude, gps.longitude);
            if dist < MIN_DISTANCE_M {
                return;
            }
        }

        // Write GPX trackpoint
        if let Some(ref mut f) = self.file {
            let point = format!(
                "      <trkpt lat=\"{:.6}\" lon=\"{:.6}\">\n        \
                 <ele>{:.1}</ele>\n        \
                 <speed>{:.1}</speed>\n        \
                 <hdop>{:.1}</hdop>\n        \
                 <sat>{}</sat>\n      \
                 </trkpt>\n",
                gps.latitude, gps.longitude,
                gps.altitude, gps.speed,
                gps.hdop, gps.satellites,
            );
            let _ = f.write_all(point.as_bytes());
            let _ = f.flush();
        }

        self.last_lat = gps.latitude;
        self.last_lon = gps.longitude;
        self.last_log_time = now;
        self.point_count += 1;
    }

    /// Close the current track file properly.
    pub fn close(&mut self) {
        if let Some(ref mut f) = self.file {
            let footer = "    </trkseg>\n  </trk>\n</gpx>\n";
            let _ = f.write_all(footer.as_bytes());
            let _ = f.flush();
        }
        self.file = None;
        self.started = false;
    }

    /// Get the current track file path.
    pub fn current_track(&self) -> Option<&PathBuf> {
        self.file_path.as_ref()
    }

    /// Get total points logged this session.
    pub fn points_logged(&self) -> u64 {
        self.point_count
    }
}

impl Drop for GpsLogger {
    fn drop(&mut self) {
        self.close();
    }
}

/// Approximate distance between two GPS points in meters.
fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6_371_000.0; // Earth radius in meters
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos()
        * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    r * c
}
