"""Top-level CLI. Subcommands are intentionally thin — they wire config,
storage, and a single domain action together. All business logic lives
in the gamelog/ and rsi/ modules.
"""
from __future__ import annotations

import json

import typer
from rich.console import Console
from rich.table import Table

from starstats.config import Config
from starstats.db import connect, init_db
from starstats.gamelog.parser import parse_line
from starstats.gamelog.tail import tail
from starstats.rsi.client import open_client
from starstats.rsi.scrape import fetch_hangar, fetch_profile

app = typer.Typer(add_completion=False, help="StarStats — personal Star Citizen metrics.")
console = Console()


@app.command()
def init() -> None:
    """Create the SQLite database in the user data directory."""
    cfg = Config.load()
    init_db(cfg.db_path)
    console.print(f"[green]Initialised[/green] {cfg.db_path}")


@app.command()
def watch() -> None:
    """Tail Game.log forever and store recognised events. Ctrl+C to stop."""
    cfg = Config.load()
    init_db(cfg.db_path)
    console.print(f"[cyan]Watching[/cyan] {cfg.gamelog_path}")
    with connect(cfg.db_path) as conn:
        for line in tail(cfg.gamelog_path, conn):
            event = parse_line(line)
            if event is None:
                continue
            conn.execute(
                "INSERT INTO events(type, timestamp, raw, payload) VALUES (?, ?, ?, ?)",
                (event.event_type, event.timestamp, event.raw, event.to_payload()),
            )
            conn.commit()
            console.print(f"[dim]{event.timestamp}[/dim] {event.event_type}")


@app.command()
def sync(handle: str = typer.Option(..., help="Your RSI handle, e.g. 'TestPilot'")) -> None:
    """Pull hangar + profile snapshots from robertsspaceindustries.com."""
    cfg = Config.load()
    init_db(cfg.db_path)
    if not cfg.rsi_token:
        console.print("[red]RSI_TOKEN missing.[/red] Copy .env.example to .env and fill it in.")
        raise typer.Exit(code=1)

    with open_client(cfg.rsi_token) as client, connect(cfg.db_path) as conn:
        hangar = fetch_hangar(client)
        profile = fetch_profile(client, handle)
        conn.execute(
            "INSERT INTO hangar_snapshot(payload) VALUES (?)", (json.dumps(hangar),),
        )
        conn.execute(
            "INSERT INTO profile_snapshot(payload) VALUES (?)", (json.dumps(profile),),
        )
    console.print("[green]Synced[/green] hangar + profile.")


@app.command()
def stats() -> None:
    """Print top-line numbers from the local DB."""
    cfg = Config.load()
    init_db(cfg.db_path)
    with connect(cfg.db_path) as conn:
        rows = conn.execute(
            "SELECT type, COUNT(*) AS n FROM events GROUP BY type ORDER BY n DESC"
        ).fetchall()

    table = Table(title="Event counts")
    table.add_column("Type")
    table.add_column("Count", justify="right")
    for r in rows:
        table.add_row(r["type"], str(r["n"]))
    console.print(table if rows else "[dim]No events captured yet.[/dim]")
