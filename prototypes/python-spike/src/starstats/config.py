"""Path and env resolution. No I/O happens here, only path math."""
from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path

from platformdirs import user_data_dir

APP_NAME = "StarStats"

DEFAULT_GAMELOG_WIN = Path(
    r"C:\Program Files\Roberts Space Industries\StarCitizen\LIVE\Game.log"
)


@dataclass(frozen=True)
class Config:
    gamelog_path: Path
    db_path: Path
    rsi_token: str | None

    @classmethod
    def load(cls) -> Config:
        data_dir = Path(user_data_dir(APP_NAME, appauthor=False))
        data_dir.mkdir(parents=True, exist_ok=True)

        gamelog = os.environ.get("STARSTATS_GAMELOG")
        db = os.environ.get("STARSTATS_DB")

        return cls(
            gamelog_path=Path(gamelog) if gamelog else DEFAULT_GAMELOG_WIN,
            db_path=Path(db) if db else data_dir / "starstats.sqlite3",
            rsi_token=os.environ.get("RSI_TOKEN") or None,
        )
