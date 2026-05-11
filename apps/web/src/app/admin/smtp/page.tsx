/**
 * Admin SMTP config page. Server component — fetches the initial
 * config via the typed (server-only) API client, defines three
 * `'use server'` actions, and hands them to the client `<SmtpForm>`
 * along with the initial state.
 *
 * The form never imports `lib/api.ts` directly because that module is
 * `server-only` (it reads `STARSTATS_API_URL` from env and threads
 * the HttpOnly session cookie's bearer). Mirrors the pattern in
 * `app/admin/submissions/_components/ModerationActions.tsx` — server
 * actions get serialised across the RPC boundary, the bearer stays
 * server-side.
 *
 * Auth: parent `/admin/layout.tsx` enforces the role gate. Defensive
 * 401 → login, 403 → /dashboard (admin-only endpoint; moderators
 * land on the nav tab but can't pass the server gate).
 */

import { redirect } from 'next/navigation';
import { revalidatePath } from 'next/cache';
import {
  ApiCallError,
  getSmtpConfig,
  putSmtpConfig,
  testSmtp,
  type SmtpConfigRequest,
  type SmtpConfigResponse,
} from '@/lib/api';
import { getSession } from '@/lib/session';
import { AdminNav } from '../_components/AdminNav';
import { SmtpForm, type ActionResult } from './_components/SmtpForm';

export default async function AdminSmtpPage() {
  const session = await getSession();
  if (!session) redirect('/auth/login?next=/admin/smtp');

  let initial: SmtpConfigResponse;
  try {
    initial = await getSmtpConfig(session.token);
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 401) {
      redirect('/auth/login?next=/admin/smtp');
    }
    if (e instanceof ApiCallError && e.status === 403) {
      redirect('/dashboard');
    }
    throw e;
  }

  async function saveAction(payload: SmtpConfigRequest): Promise<ActionResult> {
    'use server';
    const s = await getSession();
    if (!s) return { kind: 'error', message: 'no session' };
    try {
      const updated = await putSmtpConfig(payload, s.token);
      revalidatePath('/admin/smtp');
      return { kind: 'saved', config: updated };
    } catch (e) {
      if (e instanceof ApiCallError) {
        return {
          kind: 'error',
          message: `${e.body.error}${e.body.detail ? ` — ${e.body.detail}` : ''}`,
        };
      }
      return { kind: 'error', message: String(e) };
    }
  }

  async function testAction(): Promise<ActionResult> {
    'use server';
    const s = await getSession();
    if (!s) return { kind: 'error', message: 'no session' };
    try {
      const r = await testSmtp(s.token);
      return { kind: 'sent', to: r.sent_to };
    } catch (e) {
      if (e instanceof ApiCallError) {
        return {
          kind: 'error',
          message: `${e.body.error}${e.body.detail ? ` — ${e.body.detail}` : ''}`,
        };
      }
      return { kind: 'error', message: String(e) };
    }
  }

  async function reloadAction(): Promise<ActionResult> {
    'use server';
    const s = await getSession();
    if (!s) return { kind: 'error', message: 'no session' };
    try {
      const fresh = await getSmtpConfig(s.token);
      return { kind: 'reloaded', config: fresh };
    } catch (e) {
      if (e instanceof ApiCallError) {
        return {
          kind: 'error',
          message: `${e.body.error}${e.body.detail ? ` — ${e.body.detail}` : ''}`,
        };
      }
      return { kind: 'error', message: String(e) };
    }
  }

  return (
    <div
      className="ss-screen-enter"
      style={{ display: 'flex', flexDirection: 'column', gap: 20 }}
    >
      <AdminNav current="smtp" />

      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8 }}>
          Admin · SMTP transport
        </div>
        <h1
          style={{
            margin: 0,
            fontSize: 32,
            fontWeight: 600,
            letterSpacing: '-0.02em',
          }}
        >
          SMTP configuration
        </h1>
        <p
          style={{
            margin: '6px 0 0',
            color: 'var(--fg-muted)',
            fontSize: 14,
            maxWidth: 720,
          }}
        >
          The mailer hot-reloads as soon as you save — no API restart
          needed. The password is encrypted at rest using the server&apos;s
          KEK and never returned to the browser; leave the field
          blank to keep the existing password. When disabled, the
          server falls back to environment-based config (if any) or
          a no-op mailer that logs sends.
        </p>
      </header>

      <SmtpForm
        initial={initial}
        saveAction={saveAction}
        testAction={testAction}
        reloadAction={reloadAction}
      />
    </div>
  );
}
