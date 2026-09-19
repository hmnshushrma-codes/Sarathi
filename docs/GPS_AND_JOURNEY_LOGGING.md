# GPS & Journey Logging

## Architecture

```
JCR1440 Device (/st_gps.w.xml)
        │
        ▼
   TelemetryFrame { gps: GpsData, obd: ObdData }
        │
        ├──► JourneyRecorder
        │       ├── Speed source resolution (OBD → GPS → None)
        │       ├── Trip detection (auto start/stop)
        │       ├── GPS quality filtering
        │       ├── Coordinate logging → SQLite
        │       └── Trip statistics
        │
        ├──► GpsLogger (GPX backup files)
        │
        └──► ClusterLayout → Cluster renderers
                ├── Speed + source indicator
                ├── GPS coordinates
                ├── Trip distance
                └── GPS status
```

## GPS Data Source

The JCR1440 device provides GPS via its `gps_data` XML field:

| Field | Type | Description |
|---|---|---|
| speed | f32 | GPS speed (km/h) |
| latitude | f64 | Decimal degrees |
| longitude | f64 | Decimal degrees |
| altitude | f32 | Meters |
| heading | f32 | Compass bearing (degrees) |
| satellites | u8 | Visible satellites |
| hdop | f32 | Horizontal dilution of precision |
| accuracy | f32 | Position accuracy (meters) |
| fix_valid | bool | Whether GPS has a position fix |
| timestamp_ms | u64 | UTC timestamp |

No separate GPS hardware needed — the JCR1440 has a built-in GPS receiver.

## Speed Source Logic

```
OBD vehicle_speed available AND > 0.1
    → Source: OBD (most accurate, ECU-derived)

OBD unavailable, GPS fix_valid AND satellites > 0
    → Source: GPS (fallback)

Neither available
    → Source: None (show ---)
```

The cluster displays:
- Primary speed (large, center)
- Speed source label: `SPD:OBD` or `SPD:GPS`
- Secondary GPS speed (small, below primary) when OBD is primary

## Trip Detection

### Start Conditions

A trip starts when ANY of:
- Engine RPM > 100 (ignition ON)
- Speed > 3 km/h sustained for 3+ seconds

GPS drift while parked will NOT trigger a trip.

### End Conditions

A trip ends when:
- Vehicle stationary (speed < 3 km/h, no RPM) for **10 minutes**

### Sampling Strategy

| Condition | Interval |
|---|---|
| Moving (> 3 km/h) | Every 1.5 seconds, min 5m distance |
| Slow/stationary | Every 15 seconds |

### Quality Filtering

Points are rejected when:
- GPS accuracy > 100m
- Coordinates are (0, 0)
- Speed jumps > 300 km/h between samples
- Teleport: > 1km movement in < 1s at low speed

## Database Schema

SQLite database at `/var/log/ninodash/trips.db`

### trips table

| Column | Type | Description |
|---|---|---|
| id | INTEGER PK | Auto-incrementing trip ID |
| started_at | REAL | Epoch timestamp |
| ended_at | REAL | Epoch timestamp (NULL if active) |
| start_lat | REAL | Starting latitude |
| start_lng | REAL | Starting longitude |
| end_lat | REAL | Ending latitude |
| end_lng | REAL | Ending longitude |
| distance_km | REAL | Total haversine distance |
| max_gps_speed | REAL | Peak GPS speed (km/h) |
| max_obd_speed | REAL | Peak OBD speed (km/h) |
| avg_speed | REAL | Average speed (km/h) |
| point_count | INTEGER | Total GPS points logged |
| active | INTEGER | 1 if trip is ongoing |

### trip_points table

| Column | Type | Description |
|---|---|---|
| id | INTEGER PK | Auto-incrementing |
| trip_id | INTEGER FK | References trips.id |
| timestamp | REAL | Epoch timestamp |
| latitude | REAL | Decimal degrees |
| longitude | REAL | Decimal degrees |
| gps_speed | REAL | GPS speed (km/h) |
| vehicle_speed | REAL | OBD speed (km/h) |
| heading | REAL | Compass bearing |
| altitude | REAL | Meters |
| accuracy | REAL | GPS accuracy (meters) |
| satellites | INTEGER | Satellite count |

Indexes: `trip_id`, `timestamp`

## Crash Recovery

On startup, the database recovers any trips left active from a previous crash:
- Sets `active = 0`
- Sets `ended_at` to the timestamp of the last logged point
- No data is lost

WAL journal mode ensures write durability.

## Distance Calculation

Uses Haversine formula for great-circle distance between consecutive points.
Accumulated incrementally during the trip.

## Export

Trips can be exported as GPX:

```rust
journey.export_trip_gpx(trip_id) → Option<String>
```

GPX files include: lat, lon, elevation, speed per trackpoint.
Compatible with Google Earth, Garmin, OpenStreetMap tools.

Backup GPX files also written to `/var/log/ninodash/tracks/`.

## Privacy

- All GPS data stays local on the Pi
- No cloud uploads
- No reverse geocoding by default (works offline)
- Database can be deleted: `rm /var/log/ninodash/trips.db`

## CLI Tool

```bash
ninodash-trip-status
```

Output:
```
NinoDash Trip Status
========================================
Trip active: YES
Trip ID: 14
Started: 23:14:02
Points: 382
Distance: 6.7 km
Max GPS speed: 91 km/h
Max OBD speed: 88 km/h
Avg speed: 35.7 km/h
Start: 19.07598, 72.87766
Last point: 0.8 sec ago
GPS fix: YES (12 sats)

Total trips: 14
```

## Future: Route History UI

The database schema supports building a trip history screen:

```
TRIP HISTORY

19 Sep 2026
19.0760, 72.8777 → 28.6139, 77.2090

Distance: 27.4 km
Duration: 46 min
Avg 35.7 km/h

[ VIEW ROUTE ] [ EXPORT GPX ]
```

Route polylines can be rendered from trip_points.
Speed/elevation graphs from the same data.
No schema migration needed.
