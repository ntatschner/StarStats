'use server';

/**
 * Server actions used by the /sharing page. Currently just the
 * "Report a share" action (audit v2 §05 — recipient-facing report
 * affordance on each inbound-share row).
 *
 * The reporter handle is taken off the session bearer token by the
 * server — the form never sends a reporter field, so spoofing one
 * user as another is impossible from the client.
 *
 * On success we soft-refresh /sharing so any error/success surface
 * the page later grows lands without a full reload.
 */

import { revalidatePath } from 'next/cache';
import { redirect } from 'next/navigation';
import { ApiCallError, reportShare } from '@/lib/api';
import { getSession } from '@/lib/session';

const LOGIN_NEXT = '/auth/login?next=/sharing';

export async function reportShareAction(formData: FormData): Promise<void> {
  const session = await getSession();
  if (!session) redirect(LOGIN_NEXT);

  const owner_handle = String(formData.get('owner_handle') ?? '').trim();
  const recipient_handle = String(
    formData.get('recipient_handle') ?? '',
  ).trim();
  const reason = String(formData.get('reason') ?? '').trim();
  const detailsRaw = formData.get('details');
  const details =
    typeof detailsRaw === 'string' && detailsRaw.trim().length > 0
      ? detailsRaw.trim()
      : undefined;

  if (!owner_handle || !recipient_handle || !reason) {
    revalidatePath('/sharing');
    return;
  }

  try {
    await reportShare(session.token, {
      owner_handle,
      recipient_handle,
      reason,
      details,
    });
  } catch (e) {
    if (e instanceof ApiCallError) {
      if (e.status === 401) redirect(LOGIN_NEXT);
      if (e.status === 403 || e.status === 400 || e.status === 429) {
        revalidatePath('/sharing');
        return;
      }
    }
    throw e;
  }
  revalidatePath('/sharing');
}
