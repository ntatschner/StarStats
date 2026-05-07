"""Smoke tests so the harness is wired up. Replace with real coverage as
behaviour lands."""
from __future__ import annotations

import sqlite3
from pathlib import Path

from starstats import __version__
from starstats.db import init_db
from starstats.gamelog.events import ActorDeath
from starstats.gamelog.parser import parse_line


def test_version_present() -> None:
    assert __version__


def test_db_init_creates_tables(tmp_path: Path) -> None:
    db = tmp_path / "test.sqlite3"
    init_db(db)
    with sqlite3.connect(db) as conn:
        names = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    assert {"events", "hangar_snapshot", "profile_snapshot", "tail_cursor"} <= names


def test_parser_extracts_actor_death() -> None:
    line = (
        "<2024-01-15T14:30:25.832Z> [Notice] <Actor Death> CActor::Kill: "
        "'VictimName' [123456789] in zone 'OOC_Stanton_4a_PortOlisar' "
        "killed by 'KillerName' [987654321] using "
        "'Weapon_Pistol_Behring_P4AR_Default' [Class P4AR] "
        "with damage type 'Bullet' from direction x: 0.5, y: 0.2, z: -0.8\n"
    )
    event = parse_line(line)
    assert isinstance(event, ActorDeath)
    assert event.victim == "VictimName"
    assert event.killer == "KillerName"
    assert event.zone == "OOC_Stanton_4a_PortOlisar"
    assert event.weapon == "Weapon_Pistol_Behring_P4AR_Default"
    assert event.damage_type == "Bullet"
