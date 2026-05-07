"""HTML parsers for RSI account pages.

Stub implementations — selectors need to be confirmed against the live
DOM, which CIG redesigns periodically. Capture a sample page to
`tests/fixtures/` first, then build the parser test-first.
"""
from __future__ import annotations

from typing import Any

from bs4 import BeautifulSoup

from starstats.rsi.client import RsiClient


def fetch_hangar(client: RsiClient) -> dict[str, Any]:
    """Fetch and parse the user's pledge/hangar page."""
    html = client.get_html("/account/pledges")
    soup = BeautifulSoup(html, "html.parser")
    # TODO(owner): Walk `soup` and extract pledge cards. Save a fixture
    # to tests/fixtures/hangar.html first, then write a test that
    # asserts on the parsed shape, then implement here.
    return {"raw_length": len(html), "items": [], "soup_present": soup is not None}


def fetch_profile(client: RsiClient, handle: str) -> dict[str, Any]:
    """Fetch and parse the public citizen page for `handle`."""
    html = client.get_html(f"/citizens/{handle}")
    soup = BeautifulSoup(html, "html.parser")
    # TODO(owner): Extract handle, moniker, enlist date, org tag, badges.
    return {"raw_length": len(html), "handle": handle, "soup_present": soup is not None}
