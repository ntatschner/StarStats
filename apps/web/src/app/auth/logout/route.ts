import { NextRequest, NextResponse } from 'next/server';
import { clearSession } from '@/lib/session';

/**
 * GET /auth/logout — clear the session cookie and bounce to home.
 *
 * GET (rather than POST) is deliberate: the only side effect is
 * "delete *your own* cookie" and supporting plain anchor tags
 * keeps the no-JS path working.
 */
export async function GET(req: NextRequest) {
  await clearSession();
  return NextResponse.redirect(new URL('/', req.url));
}
