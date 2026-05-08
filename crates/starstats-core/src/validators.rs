//! Server-side sanity gates applied to events before they're persisted.
//!
//! Anyone can edit `Game.log` with Notepad and POST garbage at the API.
//! These validators catch the obvious cases. Statistical anomaly
//! detection lives further up the stack (in the API server's ingest
//! pipeline).

use crate::events::GameEvent;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("timestamp is empty")]
    EmptyTimestamp,
    #[error("timestamp does not look like ISO-8601")]
    BadTimestamp,
    #[error("required field {0} is empty")]
    EmptyField(&'static str),
    #[error("port {0} is out of plausible range")]
    BadPort(u16),
}

/// Lightweight validity check. Cheap to call on every ingested event.
pub fn validate_event(event: &GameEvent) -> Result<(), ValidationError> {
    let ts = match event {
        GameEvent::ProcessInit(e) => &e.timestamp,
        GameEvent::LegacyLogin(e) => &e.timestamp,
        GameEvent::JoinPu(e) => &e.timestamp,
        GameEvent::ChangeServer(e) => &e.timestamp,
        GameEvent::SeedSolarSystem(e) => &e.timestamp,
        GameEvent::ResolveSpawn(e) => &e.timestamp,
        GameEvent::ActorDeath(e) => &e.timestamp,
        GameEvent::PlayerDeath(e) => &e.timestamp,
        GameEvent::PlayerIncapacitated(e) => &e.timestamp,
        GameEvent::VehicleDestruction(e) => &e.timestamp,
        GameEvent::HudNotification(e) => &e.timestamp,
        GameEvent::LocationInventoryRequested(e) => &e.timestamp,
        GameEvent::PlanetTerrainLoad(e) => &e.timestamp,
        GameEvent::QuantumTargetSelected(e) => &e.timestamp,
        GameEvent::AttachmentReceived(e) => &e.timestamp,
        GameEvent::VehicleStowed(e) => &e.timestamp,
        GameEvent::GameCrash(e) => &e.timestamp,
        GameEvent::LauncherActivity(e) => &e.timestamp,
        GameEvent::MissionStart(e) => &e.timestamp,
        GameEvent::MissionEnd(e) => &e.timestamp,
        GameEvent::ShopBuyRequest(e) => &e.timestamp,
        GameEvent::ShopFlowResponse(e) => &e.timestamp,
        GameEvent::CommodityBuyRequest(e) => &e.timestamp,
        GameEvent::CommoditySellRequest(e) => &e.timestamp,
        GameEvent::SessionEnd(e) => &e.timestamp,
        GameEvent::RemoteMatch(e) => &e.timestamp,
    };
    check_timestamp(ts)?;

    match event {
        GameEvent::LegacyLogin(e) => {
            if e.handle.is_empty() {
                return Err(ValidationError::EmptyField("handle"));
            }
        }
        GameEvent::JoinPu(e) => {
            if e.shard.is_empty() {
                return Err(ValidationError::EmptyField("shard"));
            }
            if e.port == 0 {
                return Err(ValidationError::BadPort(e.port));
            }
        }
        GameEvent::ActorDeath(e) => {
            if e.victim.is_empty() {
                return Err(ValidationError::EmptyField("victim"));
            }
            if e.killer.is_empty() {
                return Err(ValidationError::EmptyField("killer"));
            }
        }
        GameEvent::VehicleDestruction(e) => {
            if e.vehicle_class.is_empty() {
                return Err(ValidationError::EmptyField("vehicle_class"));
            }
        }
        _ => {}
    }
    Ok(())
}

fn check_timestamp(ts: &str) -> Result<(), ValidationError> {
    if ts.is_empty() {
        return Err(ValidationError::EmptyTimestamp);
    }
    // We don't pull `chrono::DateTime::parse_from_rfc3339` here on
    // purpose — `chrono` lives in the consuming crates' deps; this
    // crate stays dependency-light. A shape check is enough.
    //
    // Accepted shapes (in order of how they reach us):
    //   - Game.log: `2026-05-02T21:14:23.189Z` (canonical ISO-8601)
    //   - GameCrash: `2026-05-04T21:10:12+00:00` (chrono `to_rfc3339`)
    //   - LauncherActivity: `2026-05-06 12:34:56.789` (Electron format,
    //     no offset, space separator).
    //
    // The relaxed check: must contain a date/time separator (`T` or
    // space) followed at some point by a colon. That catches the
    // everyday garbage cases (empty, "not-a-date", "abcdef") while
    // tolerating each upstream's preferred ISO dialect.
    let has_separator = ts.contains('T') || ts.contains(' ');
    let has_time_colon = ts.contains(':');
    if !has_separator || !has_time_colon {
        return Err(ValidationError::BadTimestamp);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::JoinPu;

    #[test]
    fn rejects_empty_shard() {
        let event = GameEvent::JoinPu(JoinPu {
            timestamp: "2026-05-02T21:14:23.189Z".into(),
            address: "1.2.3.4".into(),
            port: 64300,
            shard: String::new(),
            location_id: "1".into(),
        });
        assert_eq!(
            validate_event(&event),
            Err(ValidationError::EmptyField("shard"))
        );
    }

    #[test]
    fn accepts_valid_join_pu() {
        let event = GameEvent::JoinPu(JoinPu {
            timestamp: "2026-05-02T21:14:23.189Z".into(),
            address: "1.2.3.4".into(),
            port: 64300,
            shard: "pub_euw1b".into(),
            location_id: "1".into(),
        });
        assert_eq!(validate_event(&event), Ok(()));
    }

    #[test]
    fn rejects_bad_timestamp() {
        let event = GameEvent::JoinPu(JoinPu {
            timestamp: "not-a-date".into(),
            address: "1.2.3.4".into(),
            port: 64300,
            shard: "pub".into(),
            location_id: "1".into(),
        });
        assert_eq!(validate_event(&event), Err(ValidationError::BadTimestamp));
    }

    #[test]
    fn accepts_chrono_rfc3339_offset_form() {
        // GameCrash events carry timestamps from chrono's `to_rfc3339()`,
        // which renders UTC as `+00:00` not `Z`. The validator must
        // not reject these.
        let event = GameEvent::JoinPu(JoinPu {
            timestamp: "2026-05-04T21:10:12+00:00".into(),
            address: "1.2.3.4".into(),
            port: 64300,
            shard: "pub".into(),
            location_id: "1".into(),
        });
        assert_eq!(validate_event(&event), Ok(()));
    }

    #[test]
    fn accepts_launcher_space_separated_form() {
        // LauncherActivity carries `YYYY-MM-DD HH:MM:SS.mmm` — space
        // separator, no offset. Also acceptable.
        let event = GameEvent::JoinPu(JoinPu {
            timestamp: "2026-05-06 12:34:56.789".into(),
            address: "1.2.3.4".into(),
            port: 64300,
            shard: "pub".into(),
            location_id: "1".into(),
        });
        assert_eq!(validate_event(&event), Ok(()));
    }
}
