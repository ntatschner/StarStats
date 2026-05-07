import 'server-only';
import { cookies } from 'next/headers';

import { putPreferences } from '@/lib/api';
import { logger } from '@/lib/logger';

export type Theme = 'stanton' | 'pyro' | 'terra' | 'nyx';

export const THEMES: readonly Theme[] = ['stanton', 'pyro', 'terra', 'nyx'];
export const DEFAULT_THEME: Theme = 'stanton';
export const THEME_COOKIE = 'ss-theme';

const VALID = new Set<Theme>(THEMES);

/** ~ 1 year. Long enough that "set once" actually sticks across sessions. */
const THEME_COOKIE_MAX_AGE = 60 * 60 * 24 * 365;

export function isTheme(value: unknown): value is Theme {
  return typeof value === 'string' && VALID.has(value as Theme);
}

export async function getTheme(): Promise<Theme> {
  const store = await cookies();
  const v = store.get(THEME_COOKIE)?.value;
  return v && VALID.has(v as Theme) ? (v as Theme) : DEFAULT_THEME;
}

/**
 * Persist the user's theme choice. Sets the local `ss-theme` cookie so
 * SSR's `<html data-theme>` reflects the choice on the next render, then
 * pushes the same value to the server-side preferences row so it follows
 * the user across devices.
 *
 * The local cookie is the source of truth for paint -- if the server PUT
 * fails (network blip, 500, transient backend issue), we still want the UI
 * to honour the user's choice in this browser. The error is logged but
 * swallowed so the calling server action can complete cleanly.
 *
 * The optional `bearer` arg lets the caller forward an existing session
 * token; when omitted, the server-side persistence step is skipped (used
 * by unauthenticated flows that only need the local cookie).
 */
export async function setTheme(
  theme: Theme,
  bearer?: string,
): Promise<void> {
  if (!isTheme(theme)) {
    throw new Error(`invalid theme: ${String(theme)}`);
  }

  const store = await cookies();
  store.set(THEME_COOKIE, theme, {
    httpOnly: false,
    sameSite: 'lax',
    secure: process.env.NODE_ENV === 'production',
    path: '/',
    maxAge: THEME_COOKIE_MAX_AGE,
  });

  if (bearer) {
    try {
      await putPreferences(bearer, { theme });
    } catch (e) {
      // Cookie still wins -- degrade quietly so the local UX stays
      // consistent even when the backend is misbehaving.
      logger.warn({ err: e, theme }, 'put preferences failed');
    }
  }
}
