import Link from 'next/link';
import { DrawerToggle } from './DrawerToggle';

interface Props {
  handle: string | null;
}

/**
 * Top app bar — brand mark + handle pill on the right. Mobile renders
 * the drawer toggle on the left; tablet/desktop hide it via tokens.css.
 */
export function TopBar({ handle }: Props) {
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
      <span style={{ flex: 1 }} />
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
