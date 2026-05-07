"""Authenticated HTTP client for robertsspaceindustries.com.

Uses *your* session cookie. Never share this token. Rate limit is
deliberately conservative — CIG does throttle, and aggressive scraping
would be both rude and likely to get the cookie invalidated.
"""
from __future__ import annotations

import time
from collections.abc import Iterator
from contextlib import contextmanager

import httpx

BASE_URL = "https://robertsspaceindustries.com"
USER_AGENT = "StarStats/0.1 (personal metrics; +github.com/ntatschner/StarStats)"
MIN_INTERVAL_SECONDS = 1.0


class RsiClient:
    def __init__(self, token: str) -> None:
        if not token:
            raise ValueError("RSI_TOKEN is required (see .env.example)")
        self._token = token
        self._client = httpx.Client(
            base_url=BASE_URL,
            headers={"User-Agent": USER_AGENT},
            cookies={"Rsi-Token": token},
            timeout=30.0,
            follow_redirects=True,
        )
        self._last_request: float = 0.0

    def _throttle(self) -> None:
        wait = MIN_INTERVAL_SECONDS - (time.monotonic() - self._last_request)
        if wait > 0:
            time.sleep(wait)
        self._last_request = time.monotonic()

    def get_html(self, path: str) -> str:
        self._throttle()
        resp = self._client.get(path)
        resp.raise_for_status()
        return resp.text

    def close(self) -> None:
        self._client.close()


@contextmanager
def open_client(token: str) -> Iterator[RsiClient]:
    client = RsiClient(token)
    try:
        yield client
    finally:
        client.close()
