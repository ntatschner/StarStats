'use client';

/**
 * Client wrapper around [`QuantumWarp`] that picks a flow angle per
 * route via `usePathname()`. The angle prop flips on navigation, the
 * inner canvas tweens toward the new target — no remount.
 *
 * The bare `<QuantumWarp />` previously sat in `app/layout.tsx`,
 * which made `angle` constant at the default (180) for every screen
 * — the design intent of "each route has its own flow direction"
 * (see prototype `quantum-warp.jsx` + `app.jsx` `angleFor`) was
 * never wired up in production. This component restores that wiring.
 *
 * Static map for known routes mirrors the prototype's `FIXED_ANGLE`
 * intuition (signup → upward, donate → strong sweep, etc.) and
 * extends it for the production-only paths. Unmapped paths fall
 * through to a deterministic hash so navigating to e.g. an org
 * detail page always gives the same direction without us having
 * to enumerate every slug.
 */

import { usePathname } from 'next/navigation';
import { QuantumWarp } from './QuantumWarp';

const FIXED: Readonly<Record<string, number>> = {
  '/': 200,
  '/auth/login': 290,
  '/auth/signup': 270,
  '/auth/forgot-password': 310,
  '/auth/reset-password': 305,
  '/auth/verify': 280,
  '/auth/magic-link': 260,
  '/auth/email-change': 220,
  '/auth/totp-verify': 155,
  '/dashboard': 180,
  '/metrics': 170,
  '/submissions': 30,
  '/admin': 0,
  '/devices': 120,
  '/orgs': 60,
  '/journey': 100,
  '/uploads': 200,
  '/settings': 195,
  '/settings/2fa': 155,
  '/donate': 245,
  '/privacy': 215,
};

/** Tiny deterministic hash so any unmapped pathname gets a stable
 *  angle (same path → same direction across visits). djb2-ish. */
function hashAngle(s: string): number {
  let h = 5381;
  for (let i = 0; i < s.length; i++) {
    h = (h * 33) ^ s.charCodeAt(i);
  }
  return ((h % 360) + 360) % 360;
}

function angleFor(pathname: string): number {
  if (FIXED[pathname] != null) return FIXED[pathname];
  // Try progressively shorter prefixes so e.g. /orgs/foo inherits
  // /orgs's angle without enumerating every slug.
  const segments = pathname.split('/').filter(Boolean);
  for (let i = segments.length; i > 0; i--) {
    const prefix = '/' + segments.slice(0, i).join('/');
    if (FIXED[prefix] != null) return FIXED[prefix];
  }
  return hashAngle(pathname);
}

export function QuantumWarpBackground() {
  const pathname = usePathname();
  return <QuantumWarp angle={angleFor(pathname)} />;
}
