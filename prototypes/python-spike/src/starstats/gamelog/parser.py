"""Regex line parser for Game.log.

DESIGN NOTE FOR THE OWNER:
The regex format here is *empirical* — CIG changes the wording of log
lines between patches. Treat each pattern as a hypothesis to verify
against a real Game.log capture. The first pattern (`ACTOR_DEATH_RE`)
is based on the format observed in 3.x patches; verify before trusting.

To add a new event:
  1. Find a representative line in `tests/fixtures/sample_game_log.txt`
  2. Write a regex with named groups matching `events.py` fields
  3. Add a branch in `parse_line()` returning the populated dataclass
  4. Add a fixture-driven test in `tests/test_parser.py`
"""
from __future__ import annotations

import re

from starstats.gamelog.events import ActorDeath, GameEvent

# Example line (verify against your own Game.log):
# <2024-01-15T14:30:25.832Z> [Notice] <Actor Death> CActor::Kill: 'Victim'
#   [123] in zone 'Zone_X' killed by 'Killer' [456] using
#   'Weapon_Pistol_Default' [Class W] with damage type 'Bullet' ...
ACTOR_DEATH_RE = re.compile(
    r"<(?P<ts>[^>]+)>\s+\[Notice\]\s+<Actor Death>.*?"
    r"'(?P<victim>[^']+)'\s+\[(?P<victim_geid>\d+)\].*?"
    r"in zone\s+'(?P<zone>[^']+)'.*?"
    r"killed by\s+'(?P<killer>[^']+)'\s+\[(?P<killer_geid>\d+)\].*?"
    r"using\s+'(?P<weapon>[^']+)'.*?"
    r"with damage type\s+'(?P<damage_type>[^']+)'"
)


def parse_line(line: str) -> GameEvent | None:
    """Return a populated `GameEvent` subclass, or None for unrecognised lines."""
    m = ACTOR_DEATH_RE.search(line)
    if m:
        return ActorDeath(
            timestamp=m["ts"],
            raw=line.rstrip("\n"),
            victim=m["victim"],
            victim_geid=m["victim_geid"],
            zone=m["zone"],
            killer=m["killer"],
            killer_geid=m["killer_geid"],
            weapon=m["weapon"],
            damage_type=m["damage_type"],
        )

    # TODO(owner): add VehicleDestruction and JumpDriveStateChanged branches.
    return None
