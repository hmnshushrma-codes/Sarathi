//! Journey recorder — auto-detects trips and logs GPS + OBD data.
//!
//! Trip detection:
//!   START: speed > 3 km/h for 3+ seconds, OR RPM > 0
//!   END:   stationary for 10 minutes (configurable)
//!
//! Speed source logic:
//!   1. OBD speed if available and fresh
//!   2. GPS speed if fix valid
//!   3. None (show ---)
//!
//! Survives cluster switches — runs independently of the UI.

use std::time::Instant;

use jcr1440_client::{GpsData, ObdData};

use crate::trip_db::{TripDb, TripPoint};

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

const TRIP_START_SPEED_KPH: f32 = 3.0;
const TRIP_START_HOLD_SECS: f32 = 3.0;
const TRIP_END_TIMEOUT_SECS: f64 = 600.0; // 10 minutes
const MIN_LOG_DISTANCE_M: f64 = 5.0;
const MIN_LOG_INTERVAL_MOVING: f64 = 1.5;   // seconds
const MIN_LOG_INTERVAL_SLOW: f64 = 15.0;    // seconds when < 3 km/h
const MAX_SPEED_JUMP_KPH: f32 = 300.0;      // reject teleports
const MIN_ACCURACY_M: f32 = 100.0;          // reject bad fixes

// ---------------------------------------------------------------------------
// Speed source
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpeedSource {
    Obd,
    Gps,
    None,
}

impl SpeedSource {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Obd => "OBD",
            Self::Gps => "GPS",
            Self::None => "---",
        }
    }
}

// ---------------------------------------------------------------------------
// Journey state
// ---------------------------------------------------------------------------

pub struct JourneyRecorder {
    db: Option<TripDb>,
    // Current trip
    active_trip_id: Option<i64>,
    trip_distance_km: f64,
    trip_point_count: u32,
    trip_max_gps_speed: f32,
    trip_max_obd_speed: f32,
    trip_speed_sum: f64,
    // Last logged point
    last_lat: f64,
    last_lon: f64,
    last_log_time: f64,
    last_gps_speed: f32,
    // Trip detection
    moving_since: Option<Instant>,
    stationary_since: Option<Instant>,
    // Speed source
    pub speed_source: SpeedSource,
    pub primary_speed: f32,
    pub gps_speed: f32,
    pub obd_speed: f32,
    // GPS state
    pub gps_fix: bool,
    pub satellites: u8,
    pub latitude: f64,
    pub longitude: f64,
    pub accuracy: f32,
    pub heading: f32,
    pub altitude: f32,
    // Stats
    pub total_points_logged: u64,
    pub rejected_points: u64,
}

impl JourneyRecorder {
    pub fn new() -> Self {
        let db = match TripDb::open() {
            Ok(db) => {
                tracing::info!("Trip database opened");
                Some(db)
            }
            Err(e) => {
                tracing::error!("Failed to open trip database: {}", e);
                None
            }
        };

        Self {
            db,
            active_trip_id: None,
            trip_distance_km: 0.0,
            trip_point_count: 0,
            trip_max_gps_speed: 0.0,
            trip_max_obd_speed: 0.0,
            trip_speed_sum: 0.0,
            last_lat: 0.0,
            last_lon: 0.0,
            last_log_time: 0.0,
            last_gps_speed: 0.0,
            moving_since: None,
            stationary_since: None,
            speed_source: SpeedSource::None,
            primary_speed: 0.0,
            gps_speed: 0.0,
            obd_speed: 0.0,
            gps_fix: false,
            satellites: 0,
            latitude: 0.0,
            longitude: 0.0,
            accuracy: 0.0,
            heading: 0.0,
            altitude: 0.0,
            total_points_logged: 0,
            rejected_points: 0,
        }
    }

    /// Process a telemetry frame. Call this every update cycle.
    pub fn update(&mut self, gps: &GpsData, obd: &ObdData) {
        // Update GPS state
        self.gps_fix = gps.fix_valid;
        self.satellites = gps.satellites;
        self.latitude = gps.latitude;
        self.longitude = gps.longitude;
        self.accuracy = gps.accuracy;
        self.heading = gps.heading;
        self.altitude = gps.altitude;
        self.gps_speed = gps.speed;
        self.obd_speed = obd.vehicle_speed.unwrap_or(0.0);

        // Resolve speed source
        let has_obd = obd.vehicle_speed.is_some() && self.obd_speed > 0.1;
        let has_gps = gps.fix_valid && gps.satellites > 0;

        if has_obd {
            self.speed_source = SpeedSource::Obd;
            self.primary_speed = self.obd_speed;
        } else if has_gps {
            self.speed_source = SpeedSource::Gps;
            self.primary_speed = gps.speed;
        } else {
            self.speed_source = SpeedSource::None;
            self.primary_speed = 0.0;
        }

        // Trip detection
        let effective_speed = self.primary_speed;
        let is_moving = effective_speed > TRIP_START_SPEED_KPH;
        let has_rpm = obd.engine_rpm.map_or(false, |r| r > 100.0);

        if is_moving || has_rpm {
            self.stationary_since = None;
            if self.moving_since.is_none() {
                self.moving_since = Some(Instant::now());
            }
        } else {
            self.moving_since = None;
            if self.stationary_since.is_none() {
                self.stationary_since = Some(Instant::now());
            }
        }

        // Start trip
        if self.active_trip_id.is_none() {
            let should_start = has_rpm || self.moving_since
                .map_or(false, |t| t.elapsed().as_secs_f32() > TRIP_START_HOLD_SECS);

            if should_start {
                self.start_trip(gps);
            }
        }

        // End trip (stationary timeout)
        if self.active_trip_id.is_some() {
            if let Some(since) = self.stationary_since {
                if since.elapsed().as_secs_f64() > TRIP_END_TIMEOUT_SECS {
                    self.end_trip();
                }
            }
        }

        // Log point if trip active
        if self.active_trip_id.is_some() && gps.fix_valid && gps.satellites > 0 {
            self.maybe_log_point(gps, obd);
        }
    }

    fn start_trip(&mut self, gps: &GpsData) {
        if let Some(ref db) = self.db {
            match db.start_trip(gps.latitude, gps.longitude) {
                Ok(id) => {
                    self.active_trip_id = Some(id);
                    self.trip_distance_km = 0.0;
                    self.trip_point_count = 0;
                    self.trip_max_gps_speed = 0.0;
                    self.trip_max_obd_speed = 0.0;
                    self.trip_speed_sum = 0.0;
                    self.last_lat = gps.latitude;
                    self.last_lon = gps.longitude;
                    self.last_log_time = 0.0;
                    tracing::info!("Trip {} started", id);
                }
                Err(e) => tracing::error!("Failed to start trip: {}", e),
            }
        }
    }

    fn end_trip(&mut self) {
        if let (Some(id), Some(ref db)) = (self.active_trip_id, &self.db) {
            // Update final stats
            let avg = if self.trip_point_count > 0 {
                (self.trip_speed_sum / self.trip_point_count as f64) as f32
            } else {
                0.0
            };
            let _ = db.update_trip_stats(
                id, self.trip_distance_km,
                self.trip_max_gps_speed, self.trip_max_obd_speed,
                avg, self.trip_point_count,
                self.last_lat, self.last_lon,
            );
            let _ = db.end_trip(id);
            tracing::info!(
                "Trip {} ended: {:.1}km, {} points",
                id, self.trip_distance_km, self.trip_point_count
            );
        }
        self.active_trip_id = None;
    }

    fn maybe_log_point(&mut self, gps: &GpsData, obd: &ObdData) {
        let now = now_secs();

        // Quality filters
        if gps.accuracy > MIN_ACCURACY_M && gps.accuracy < 9999000.0 {
            self.rejected_points += 1;
            return;
        }
        if gps.latitude == 0.0 && gps.longitude == 0.0 {
            return;
        }

        // Speed jump filter (teleport detection)
        if self.trip_point_count > 0 {
            let speed_diff = (gps.speed - self.last_gps_speed).abs();
            if speed_diff > MAX_SPEED_JUMP_KPH {
                self.rejected_points += 1;
                return;
            }
        }

        // Time + distance sampling
        let dt = now - self.last_log_time;
        let is_slow = self.primary_speed < TRIP_START_SPEED_KPH;
        let min_interval = if is_slow { MIN_LOG_INTERVAL_SLOW } else { MIN_LOG_INTERVAL_MOVING };

        if dt < min_interval && self.trip_point_count > 0 {
            return;
        }

        // Distance check
        if self.trip_point_count > 0 {
            let dist = haversine_m(self.last_lat, self.last_lon, gps.latitude, gps.longitude);

            // Teleport filter: > 1km in < 1s at low speed
            if dist > 1000.0 && dt < 1.0 && self.primary_speed < 100.0 {
                self.rejected_points += 1;
                return;
            }

            if dist < MIN_LOG_DISTANCE_M && dt < min_interval * 2.0 {
                return;
            }

            self.trip_distance_km += dist / 1000.0;
        }

        // Log the point
        let trip_id = match self.active_trip_id {
            Some(id) => id,
            None => return,
        };

        let point = TripPoint {
            timestamp: now,
            latitude: gps.latitude,
            longitude: gps.longitude,
            gps_speed: gps.speed,
            vehicle_speed: obd.vehicle_speed.unwrap_or(0.0),
            heading: gps.heading,
            altitude: gps.altitude,
            accuracy: gps.accuracy,
            satellites: gps.satellites,
        };

        if let Some(ref db) = self.db {
            if let Err(e) = db.log_point(trip_id, &point) {
                tracing::error!("Failed to log point: {}", e);
                return;
            }
        }

        // Update stats
        self.trip_point_count += 1;
        self.total_points_logged += 1;
        if gps.speed > self.trip_max_gps_speed {
            self.trip_max_gps_speed = gps.speed;
        }
        let obd_spd = obd.vehicle_speed.unwrap_or(0.0);
        if obd_spd > self.trip_max_obd_speed {
            self.trip_max_obd_speed = obd_spd;
        }
        self.trip_speed_sum += self.primary_speed as f64;

        self.last_lat = gps.latitude;
        self.last_lon = gps.longitude;
        self.last_log_time = now;
        self.last_gps_speed = gps.speed;

        // Periodically update trip stats in DB (every 20 points)
        if self.trip_point_count % 20 == 0 {
            if let Some(ref db) = self.db {
                let avg = (self.trip_speed_sum / self.trip_point_count as f64) as f32;
                let _ = db.update_trip_stats(
                    trip_id, self.trip_distance_km,
                    self.trip_max_gps_speed, self.trip_max_obd_speed,
                    avg, self.trip_point_count,
                    self.last_lat, self.last_lon,
                );
            }
        }
    }

    // -- Public getters --

    pub fn is_trip_active(&self) -> bool {
        self.active_trip_id.is_some()
    }

    pub fn trip_id(&self) -> Option<i64> {
        self.active_trip_id
    }

    pub fn trip_distance(&self) -> f64 {
        self.trip_distance_km
    }

    pub fn trip_points(&self) -> u32 {
        self.trip_point_count
    }

    pub fn trip_started_at(&self) -> Option<f64> {
        self.active_trip_id.and_then(|id| {
            self.db.as_ref()?.latest_trip().ok()?.filter(|t| t.id == id).map(|t| t.started_at)
        })
    }

    /// Export a trip as GPX.
    pub fn export_trip_gpx(&self, trip_id: i64) -> Option<String> {
        self.db.as_ref()?.export_gpx(trip_id).ok()
    }

    /// Force-end the current trip (e.g., on app shutdown).
    pub fn force_end_trip(&mut self) {
        if self.active_trip_id.is_some() {
            self.end_trip();
        }
    }
}

impl Drop for JourneyRecorder {
    fn drop(&mut self) {
        self.force_end_trip();
    }
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6_371_000.0;
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos()
        * (dlon / 2.0).sin().powi(2);
    r * 2.0 * a.sqrt().atan2((1.0 - a).sqrt())
}
