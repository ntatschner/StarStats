import Link from 'next/link';
import type { ResolvedLocation } from '@/lib/api';
import { LocationChip } from '@/components/LocationPill';
import { DrawerToggle } from './DrawerToggle';

interface Props {
  handle: string | null;
  /**
   * Most recent in-game location, or null when the server reported
   * no recent activity (204) or the fetch failed. Surfaced as a
   * compact chip beside the brand so the user always knows their
   * current grounding without leaving the page.
   */
  location?: ResolvedLocation | null;
}

/**
 * Top app bar — brand mark + location chip + global event search +
 * handle pill. Sticky-positioned (see `.ss-topbar` in
 * `starstats-tokens.css`) so it stays visible while the main scroll
 * container moves underneath. Mobile renders the drawer toggle on
 * the left; tablet/desktop hide it via tokens.css.
 */
export function TopBar({ handle, location = null }: Props) {
  return (
    <header className="ss-topbar">
      <DrawerToggle />
      <Link
        href="/dashboard"
        className="ss-mark"
        style={{ textDecoration: 'none' }}
      >
        <span className="ss-mark-glyph mono">★</span>
        <span>STARSTATS</span>
      </Link>
      <LocationChip location={location} />
      <span style={{ flex: 1 }} />
      {/* Native GET form — submits to /journey?view=types&type=<input>,
          the merged event-type filter surface (audit v2 §07 moved the
          old /metrics raw stream here). Server-rendered so it works
          without JS. */}
      <form
        method="GET"
        action="/journey"
        role="search"
        style={{ display: 'flex', alignItems: 'center', gap: 6 }}
      >
        <input type="hidden" name="view" value="types" />
        <input
          type="search"
          name="type"
          placeholder="Filter events…"
          aria-label="Filter events by type"
          autoComplete="off"
          spellCheck={false}
          className="mono"
          style={{
            width: 180,
            maxWidth: '32vw',
            padding: '6px 10px',
            fontSize: 12,
            background: 'var(--bg-elev)',
            border: '1px solid var(--border)',
            borderRadius: 'var(--r-sm)',
            color: 'var(--fg)',
            outlineColor: 'var(--accent)',
          }}
        />
      </form>
      {handle && (
        <span
          className="mono"
          style={{ color: 'var(--fg-muted)', fontSize: 13 }}
        >
          @{handle}
        </span>
      )}
    </header>
  );
}
