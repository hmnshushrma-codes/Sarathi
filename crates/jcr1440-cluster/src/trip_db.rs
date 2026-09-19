//! SQLite trip database — stores journey data for route history.
//!
//! Schema:
//!   trips       — one row per journey (start/end, distance, stats)
//!   trip_points — GPS coordinates logged during each trip
//!
//! All data stays local. No cloud uploads.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, Result as SqlResult};

const DB_DIR: &str = "/var/log/ninodash";
const DB_NAME: &str = "trips.db";

pub struct TripDb {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct Trip {
    pub id: i64,
    pub started_at: f64,
    pub ended_at: Option<f64>,
    pub start_lat: f64,
    pub start_lng: f64,
    pub end_lat: f64,
    pub end_lng: f64,
    pub distance_km: f64,
    pub max_gps_speed: f32,
    pub max_obd_speed: f32,
    pub avg_speed: f32,
    pub point_count: u32,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct TripPoint {
    pub timestamp: f64,
    pub latitude: f64,
    pub longitude: f64,
    pub gps_speed: f32,
    pub vehicle_speed: f32,
    pub heading: f32,
    pub altitude: f32,
    pub accuracy: f32,
    pub satellites: u8,
}

impl TripDb {
    pub fn open() -> SqlResult<Self> {
        let _ = std::fs::create_dir_all(DB_DIR);
        let path = Path::new(DB_DIR).join(DB_NAME);
        let conn = Connection::open(&path)?;

        // WAL mode for concurrent reads + crash safety
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS trips (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                started_at REAL NOT NULL,
                ended_at REAL,
                start_lat REAL NOT NULL DEFAULT 0,
                start_lng REAL NOT NULL DEFAULT 0,
                end_lat REAL NOT NULL DEFAULT 0,
                end_lng REAL NOT NULL DEFAULT 0,
                distance_km REAL NOT NULL DEFAULT 0,
                max_gps_speed REAL NOT NULL DEFAULT 0,
                max_obd_speed REAL NOT NULL DEFAULT 0,
                avg_speed REAL NOT NULL DEFAULT 0,
                point_count INTEGER NOT NULL DEFAULT 0,
                active INTEGER NOT NULL DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS trip_points (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                trip_id INTEGER NOT NULL,
                timestamp REAL NOT NULL,
                latitude REAL NOT NULL,
                longitude REAL NOT NULL,
                gps_speed REAL NOT NULL DEFAULT 0,
                vehicle_speed REAL NOT NULL DEFAULT 0,
                heading REAL NOT NULL DEFAULT 0,
                altitude REAL NOT NULL DEFAULT 0,
                accuracy REAL NOT NULL DEFAULT 0,
                satellites INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY (trip_id) REFERENCES trips(id)
            );

            CREATE INDEX IF NOT EXISTS idx_points_trip ON trip_points(trip_id);
            CREATE INDEX IF NOT EXISTS idx_points_ts ON trip_points(timestamp);",
        )?;

        // Recover any trips left active from a crash
        let db = Self { conn };
        db.recover_stale_trips()?;
        Ok(db)
    }

    /// Close any trips that were left active (crash recovery).
    fn recover_stale_trips(&self) -> SqlResult<()> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM trips WHERE active = 1", [], |r| r.get(0))?;
        if count > 0 {
            // Close stale trips using last point timestamp as end time
            self.conn.execute(
                "UPDATE trips SET
                    active = 0,
                    ended_at = COALESCE(
                        (SELECT MAX(timestamp) FROM trip_points WHERE trip_id = trips.id),
                        started_at
                    )
                WHERE active = 1", [])?;
            tracing::info!("Recovered {} stale trip(s) from previous session", count);
        }
        Ok(())
    }

    /// Start a new trip. Returns trip ID.
    pub fn start_trip(&self, lat: f64, lng: f64) -> SqlResult<i64> {
        let now = now_epoch();
        self.conn.execute(
            "INSERT INTO trips (started_at, start_lat, start_lng) VALUES (?1, ?2, ?3)",
            params![now, lat, lng],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Log a GPS point for an active trip.
    pub fn log_point(&self, trip_id: i64, point: &TripPoint) -> SqlResult<()> {
        self.conn.execute(
            "INSERT INTO trip_points
                (trip_id, timestamp, latitude, longitude, gps_speed, vehicle_speed,
                 heading, altitude, accuracy, satellites)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                trip_id, point.timestamp, point.latitude, point.longitude,
                point.gps_speed, point.vehicle_speed, point.heading,
                point.altitude, point.accuracy, point.satellites as i32,
            ],
        )?;
        Ok(())
    }

    /// Update trip running statistics.
    pub fn update_trip_stats(
        &self, trip_id: i64,
        distance_km: f64, max_gps: f32, max_obd: f32,
        avg_speed: f32, point_count: u32,
        end_lat: f64, end_lng: f64,
    ) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE trips SET
                distance_km = ?2, max_gps_speed = ?3, max_obd_speed = ?4,
                avg_speed = ?5, point_count = ?6, end_lat = ?7, end_lng = ?8
            WHERE id = ?1",
            params![trip_id, distance_km, max_gps, max_obd, avg_speed, point_count, end_lat, end_lng],
        )?;
        Ok(())
    }

    /// End an active trip.
    pub fn end_trip(&self, trip_id: i64) -> SqlResult<()> {
        let now = now_epoch();
        self.conn.execute(
            "UPDATE trips SET active = 0, ended_at = ?2 WHERE id = ?1",
            params![trip_id, now],
        )?;
        Ok(())
    }

    /// Get the most recent trip.
    pub fn latest_trip(&self) -> SqlResult<Option<Trip>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, started_at, ended_at, start_lat, start_lng, end_lat, end_lng,
                    distance_km, max_gps_speed, max_obd_speed, avg_speed, point_count, active
             FROM trips ORDER BY id DESC LIMIT 1")?;
        let mut rows = stmt.query_map([], |row| {
            Ok(Trip {
                id: row.get(0)?,
                started_at: row.get(1)?,
                ended_at: row.get(2)?,
                start_lat: row.get(3)?,
                start_lng: row.get(4)?,
                end_lat: row.get(5)?,
                end_lng: row.get(6)?,
                distance_km: row.get(7)?,
                max_gps_speed: row.get(8)?,
                max_obd_speed: row.get(9)?,
                avg_speed: row.get(10)?,
                point_count: row.get(11)?,
                active: row.get::<_, i32>(12)? != 0,
            })
        })?;
        Ok(rows.next().and_then(|r| r.ok()))
    }

    /// Export a trip as GPX string.
    pub fn export_gpx(&self, trip_id: i64) -> SqlResult<String> {
        let mut gpx = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <gpx version=\"1.1\" creator=\"NinoDash\">\n\
             <trk><name>Trip</name><trkseg>\n");

        let mut stmt = self.conn.prepare(
            "SELECT latitude, longitude, altitude, gps_speed, accuracy, satellites, timestamp
             FROM trip_points WHERE trip_id = ?1 ORDER BY timestamp")?;
        let rows = stmt.query_map(params![trip_id], |row| {
            Ok((
                row.get::<_, f64>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, f32>(2)?,
                row.get::<_, f32>(3)?,
                row.get::<_, f32>(4)?,
                row.get::<_, i32>(5)?,
            ))
        })?;

        for row in rows {
            let (lat, lon, alt, spd, _acc, _sat) = row?;
            gpx.push_str(&format!(
                "<trkpt lat=\"{:.6}\" lon=\"{:.6}\"><ele>{:.1}</ele><speed>{:.1}</speed></trkpt>\n",
                lat, lon, alt, spd));
        }

        gpx.push_str("</trkseg></trk></gpx>\n");
        Ok(gpx)
    }

    /// Get total trip count.
    pub fn trip_count(&self) -> SqlResult<i64> {
        self.conn.query_row("SELECT COUNT(*) FROM trips", [], |r| r.get(0))
    }
}

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
