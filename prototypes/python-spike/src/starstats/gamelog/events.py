"""Event dataclasses for parsed Game.log lines.

DESIGN NOTE FOR THE OWNER:
Add an event subclass for every kind of line you want to track. Each
class needs:

  - `event_type: str` — short stable id stored in `events.type` column
  - The fields you care about, all serialisable by `to_payload()`

`ActorDeath` is fully written as a worked example. The other two are
TODO stubs — they're the spots in the codebase where your domain
knowledge of Star Citizen events shapes the schema.
"""
from __future__ import annotations

import json
from dataclasses import asdict, dataclass


@dataclass(frozen=True, kw_only=True)
class GameEvent:
    event_type: str
    timestamp: str  # ISO8601 from the log line
    raw: str

    def to_payload(self) -> str:
        d = asdict(self)
        d.pop("event_type", None)
        d.pop("timestamp", None)
        d.pop("raw", None)
        return json.dumps(d, separators=(",", ":"))


@dataclass(frozen=True, kw_only=True)
class ActorDeath(GameEvent):
    """`<Actor Death>` line. Fires for every NPC and player death."""

    event_type: str = "actor_death"
    victim: str = ""
    victim_geid: str = ""
    zone: str = ""
    killer: str = ""
    killer_geid: str = ""
    weapon: str = ""
    damage_type: str = ""


# TODO(owner): Fill these in once you've decided what fields matter.
# Look at a real Game.log line, copy the relevant chunks into fields,
# then write the regex in parser.py to populate them.
@dataclass(frozen=True, kw_only=True)
class VehicleDestruction(GameEvent):
    """`<Vehicle Destruction>` line. Ship/vehicle blown up."""

    event_type: str = "vehicle_destruction"
    # TODO: vehicle_class, owner, destroy_level, caused_by, zone, ...


@dataclass(frozen=True, kw_only=True)
class JumpDriveStateChanged(GameEvent):
    """`<Jump Drive State Changed>` line. Useful for travel telemetry."""

    event_type: str = "jump_drive_state"
    # TODO: vehicle, old_state, new_state, ...
