import { useEffect, useState } from 'react';
import { api, type HealthItem } from '../api';

const POLL_MS = 15_000;

/**
 * Polls `get_health` on a 15s cadence (matching the existing status
 * poll). Returns the most-recent successful list plus a refresh
 * function. Failures are silenced — the Health UI is non-blocking,
 * and per-call surfacing would be noisy. The next successful poll
 * supersedes the silent failure.
 */
export function useHealth(): { items: HealthItem[]; refresh: () => Promise<void> } {
  const [items, setItems] = useState<HealthItem[]>([]);

  const refresh = async () => {
    try {
      const next = await api.getHealth();
      setItems(next);
    } catch {
      // intentionally swallowed; see fn doc
    }
  };

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      if (cancelled) return;
      await refresh();
    })();
    const handle = window.setInterval(() => {
      if (!cancelled) void refresh();
    }, POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(handle);
    };
  }, []);

  return { items, refresh };
}
