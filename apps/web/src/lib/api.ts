/**
 * Thin wrapper around the StarStats API. Server-side only —
 * everything here runs in Server Components / Server Actions /
 * route handlers and the JWT never reaches the browser-side JS.
 *
 * The API URL is configured via STARSTATS_API_URL (server env).
 * In dev that's typically `http://localhost:8080`; in prod it
 * lives behind Traefik at `https://api.example.com`.
 *
 * Type contract: every response/request shape is a type alias over
 * the generated OpenAPI schema, imported as a workspace dep from the
 * `api-client-ts` package (sourced from
 * `packages/api-client-ts/src/generated/schema.ts`). The exported
 * type names here are kept stable so existing call sites don't churn.
 * To regenerate after server changes:
 *   pnpm --filter api-client-ts run generate
 */

import 'server-only';
import type { components as apiSchema } from 'api-client-ts';

// Every response/request shape below is sourced from the generated
// OpenAPI schema (`packages/api-client-ts/src/generated/schema.ts`)
// rather than hand-rolled. The local `export type` names are kept
// stable so existing call sites don't churn — they're just aliases
// pointing at the codegen output. To regenerate after server changes:
//   pnpm --filter api-client-ts run generate
export type SummaryResponse = apiSchema['schemas']['SummaryResponse'];

export type AuthResponse = apiSchema['schemas']['AuthResponse'];

export type MeResponse = apiSchema['schemas']['MeResponse'];

export type ChangePasswordRequest =
  apiSchema['schemas']['ChangePasswordRequest'];
export type ChangePasswordResponse =
  apiSchema['schemas']['ChangePasswordResponse'];

export type DeleteAccountRequest =
  apiSchema['schemas']['DeleteAccountRequest'];
export type DeleteAccountResponse =
  apiSchema['schemas']['DeleteAccountResponse'];

export type ResendVerificationResponse =
  apiSchema['schemas']['ResendVerificationResponse'];

export type PasswordResetStartRequest =
  apiSchema['schemas']['PasswordResetStartRequest'];
export type PasswordResetStartResponse =
  apiSchema['schemas']['PasswordResetStartResponse'];
export type PasswordResetCompleteRequest =
  apiSchema['schemas']['PasswordResetCompleteRequest'];
export type PasswordResetCompleteResponse =
  apiSchema['schemas']['PasswordResetCompleteResponse'];

export type EmailChangeStartRequest =
  apiSchema['schemas']['EmailChangeStartRequest'];
export type EmailChangeStartResponse =
  apiSchema['schemas']['EmailChangeStartResponse'];
export type EmailChangeVerifyRequest =
  apiSchema['schemas']['EmailChangeVerifyRequest'];
export type EmailChangeVerifyResponse =
  apiSchema['schemas']['EmailChangeVerifyResponse'];

export type RsiStartResponse = apiSchema['schemas']['RsiStartResponse'];
export type RsiVerifyResponse = apiSchema['schemas']['RsiVerifyResponse'];

export type MagicLinkStartRequest =
  apiSchema['schemas']['MagicLinkStartRequest'];
export type MagicLinkStartResponse =
  apiSchema['schemas']['MagicLinkStartResponse'];
export type MagicLinkRedeemRequest =
  apiSchema['schemas']['MagicLinkRedeemRequest'];

export type TotpSetupResponse = apiSchema['schemas']['TotpSetupResponse'];
export type TotpConfirmRequest = apiSchema['schemas']['TotpConfirmRequest'];
export type TotpConfirmResponse = apiSchema['schemas']['TotpConfirmResponse'];
export type TotpDisableRequest = apiSchema['schemas']['TotpDisableRequest'];
export type TotpDisableResponse =
  apiSchema['schemas']['TotpDisableResponse'];
export type RegenerateRecoveryRequest =
  apiSchema['schemas']['RegenerateRecoveryRequest'];
export type RegenerateRecoveryResponse =
  apiSchema['schemas']['RegenerateRecoveryResponse'];
export type VerifyLoginRequest =
  apiSchema['schemas']['VerifyLoginRequest'];

export type TimelineBucket = apiSchema['schemas']['TimelineBucket'];
export type TimelineResponse = apiSchema['schemas']['TimelineResponse'];

// `PairingResponse` is the local name; the generated schema calls the
// same shape `StartResponse` (it's the body of POST /v1/auth/devices/start).
// The alias preserves the existing import name in callers.
export type PairingResponse = apiSchema['schemas']['StartResponse'];

export type DeviceListResponse = apiSchema['schemas']['DeviceListResponse'];

export type DeviceDto = apiSchema['schemas']['DeviceDto'];

export type VerifyEmailResponse = apiSchema['schemas']['VerifyEmailResponse'];

export type SmtpConfigResponse = apiSchema['schemas']['SmtpConfigResponse'];
export type SmtpConfigRequest = apiSchema['schemas']['SmtpConfigRequest'];
export type TestSendResponse = apiSchema['schemas']['TestSendResponse'];

export interface ApiError {
  error: string;
  detail?: string;
}

export class ApiCallError extends Error {
  constructor(
    public readonly status: number,
    public readonly body: ApiError,
  ) {
    super(`${status} ${body.error}${body.detail ? ` — ${body.detail}` : ''}`);
    this.name = 'ApiCallError';
  }
}

export function apiBase(): string {
  const raw = process.env.STARSTATS_API_URL;
  if (!raw) {
    throw new Error(
      'STARSTATS_API_URL is not set — point it at the Rust API origin',
    );
  }
  return raw.replace(/\/+$/, '');
}

async function request<T>(
  method: 'GET' | 'POST' | 'PUT' | 'DELETE',
  path: string,
  body: unknown | undefined,
  bearer: string | undefined,
): Promise<T> {
  const headers: Record<string, string> = {};
  if (body !== undefined) headers['content-type'] = 'application/json';
  if (bearer) headers.authorization = `Bearer ${bearer}`;

  const resp = await fetch(`${apiBase()}${path}`, {
    method,
    headers,
    body: body !== undefined ? JSON.stringify(body) : undefined,
    cache: 'no-store',
  });

  if (resp.status === 204) {
    return undefined as T;
  }

  if (!resp.ok) {
    let errBody: ApiError;
    try {
      errBody = (await resp.json()) as ApiError;
    } catch {
      errBody = { error: `http_${resp.status}` };
    }
    throw new ApiCallError(resp.status, errBody);
  }

  return (await resp.json()) as T;
}

async function postJson<T>(
  path: string,
  body: unknown,
  bearer?: string,
): Promise<T> {
  return request<T>('POST', path, body, bearer);
}

async function putJson<T>(
  path: string,
  body: unknown,
  bearer?: string,
): Promise<T> {
  return request<T>('PUT', path, body, bearer);
}

export async function signup(input: {
  email: string;
  password: string;
  claimed_handle: string;
}): Promise<AuthResponse> {
  return postJson<AuthResponse>('/v1/auth/signup', input);
}

export async function login(input: {
  email: string;
  password: string;
}): Promise<AuthResponse> {
  return postJson<AuthResponse>('/v1/auth/login', input);
}

export async function verifyEmail(input: {
  token: string;
}): Promise<VerifyEmailResponse> {
  return postJson<VerifyEmailResponse>('/v1/auth/email/verify', input);
}

export async function startPairing(
  bearer: string,
  input: { label?: string },
): Promise<PairingResponse> {
  return postJson<PairingResponse>('/v1/auth/devices/start', input, bearer);
}

export async function listDevices(bearer: string): Promise<DeviceListResponse> {
  return request<DeviceListResponse>(
    'GET',
    '/v1/auth/devices',
    undefined,
    bearer,
  );
}

export async function revokeDevice(
  bearer: string,
  deviceId: string,
): Promise<void> {
  await request<void>(
    'DELETE',
    `/v1/auth/devices/${encodeURIComponent(deviceId)}`,
    undefined,
    bearer,
  );
}

// -- Read-side query API --------------------------------------------
// `EventDto` is sourced from the generated OpenAPI schema, but we
// tighten two fields that the codegen emits as optional even though
// the server always populates them (utoipa can't express "nullable
// but required" cleanly, so it falls back to optional + nullable).
// Treating them as required-nullable here matches the runtime wire
// contract and keeps consumers (dashboard, formatters) honest. The
// `payload` slot is widened back to `unknown` because the generated
// `Record<string, never>` is the codegen's stand-in for free-form
// JSON, not an actually-empty object.
export type EventDto = Omit<
  apiSchema['schemas']['EventDto'],
  'event_timestamp' | 'payload'
> & {
  event_timestamp: string | null;
  payload: unknown;
};

// Local mirror of the server's `EventsListResponse` schema. We declare
// it directly rather than aliasing the generated type so we can use
// the locally-tightened `EventDto` (which types `payload: unknown`
// rather than the schema's `Record<string, never>`) and pin
// `next_after` as required-nullable — the server always emits the
// field, with `null` when there's no next page.
export interface ListEventsResponse {
  events: EventDto[];
  next_after: number | null;
}

export async function getSummary(bearer: string): Promise<SummaryResponse> {
  return request<SummaryResponse>('GET', '/v1/me/summary', undefined, bearer);
}

export interface ListEventsParams {
  /** Legacy forward cursor — superseded by after_seq. */
  after?: number;
  /** Older-page cursor: events with seq < before_seq, DESC by seq. */
  before_seq?: number;
  /** Newer-page cursor: events with seq > after_seq, ASC by seq. */
  after_seq?: number;
  /** Filter by event type (validated server-side as [a-z0-9_]{1,64}). */
  event_type?: string;
  /** ISO-8601 lower bound on event_timestamp. */
  since?: string;
  /** ISO-8601 upper bound on event_timestamp. */
  until?: string;
  limit?: number;
}

export async function listEvents(
  bearer: string,
  params: ListEventsParams = {},
): Promise<ListEventsResponse> {
  const qs = new URLSearchParams();
  if (params.after !== undefined) qs.set('after', String(params.after));
  if (params.before_seq !== undefined)
    qs.set('before_seq', String(params.before_seq));
  if (params.after_seq !== undefined)
    qs.set('after_seq', String(params.after_seq));
  if (params.event_type !== undefined && params.event_type !== '')
    qs.set('event_type', params.event_type);
  if (params.since !== undefined && params.since !== '')
    qs.set('since', params.since);
  if (params.until !== undefined && params.until !== '')
    qs.set('until', params.until);
  if (params.limit !== undefined) qs.set('limit', String(params.limit));
  const suffix = qs.toString() ? `?${qs.toString()}` : '';
  return request<ListEventsResponse>(
    'GET',
    `/v1/me/events${suffix}`,
    undefined,
    bearer,
  );
}

export type HideToggleResponse =
  apiSchema['schemas']['HideToggleResponse'];

/** Hide one event from shared/public views. Owner-only — the server
 *  filters by claimed_handle = caller. Idempotent: `changed=false`
 *  when the row was already hidden (or doesn't belong to you). */
export async function hideEvent(
  bearer: string,
  seq: number,
): Promise<HideToggleResponse> {
  return request<HideToggleResponse>(
    'POST',
    `/v1/me/events/${seq}/hide`,
    undefined,
    bearer,
  );
}

/** Reverse of {@link hideEvent} — clears `hidden_at`. Same idempotent
 *  semantics. */
export async function unhideEvent(
  bearer: string,
  seq: number,
): Promise<HideToggleResponse> {
  return request<HideToggleResponse>(
    'DELETE',
    `/v1/me/events/${seq}/hide`,
    undefined,
    bearer,
  );
}

// -- Account ---------------------------------------------------------

export async function getMe(bearer: string): Promise<MeResponse> {
  return request<MeResponse>('GET', '/v1/auth/me', undefined, bearer);
}

export async function changePassword(
  bearer: string,
  body: ChangePasswordRequest,
): Promise<ChangePasswordResponse> {
  return postJson<ChangePasswordResponse>('/v1/auth/me/password', body, bearer);
}

export async function resendVerification(
  bearer: string,
): Promise<ResendVerificationResponse> {
  return postJson<ResendVerificationResponse>(
    '/v1/auth/email/resend',
    {},
    bearer,
  );
}

export async function deleteAccount(
  bearer: string,
  body: DeleteAccountRequest,
): Promise<DeleteAccountResponse> {
  return request<DeleteAccountResponse>(
    'DELETE',
    '/v1/auth/me',
    body,
    bearer,
  );
}

// -- Password reset (unauthenticated) -------------------------------
//
// `start` always returns 200 even on miss (anti-enumeration); the
// caller must treat success as "if your address exists, an email is
// on the way." `complete` consumes the token, hashes the new
// password, and the server revokes all device JWTs server-side.

export async function passwordResetStart(
  body: PasswordResetStartRequest,
): Promise<PasswordResetStartResponse> {
  return postJson<PasswordResetStartResponse>(
    '/v1/auth/password/reset/start',
    body,
  );
}

export async function passwordResetComplete(
  body: PasswordResetCompleteRequest,
): Promise<PasswordResetCompleteResponse> {
  return postJson<PasswordResetCompleteResponse>(
    '/v1/auth/password/reset/complete',
    body,
  );
}

// -- Email change ---------------------------------------------------
//
// `start` is authenticated: the active session names a new address,
// the server stashes it on `pending_email` and emails a token there.
// `verify` is unauthenticated because users follow the link straight
// from the inbox; the token is the auth.

export async function emailChangeStart(
  bearer: string,
  body: EmailChangeStartRequest,
): Promise<EmailChangeStartResponse> {
  return postJson<EmailChangeStartResponse>(
    '/v1/auth/email/change/start',
    body,
    bearer,
  );
}

export async function emailChangeVerify(
  body: EmailChangeVerifyRequest,
): Promise<EmailChangeVerifyResponse> {
  return postJson<EmailChangeVerifyResponse>(
    '/v1/auth/email/change/verify',
    body,
  );
}

// -- RSI handle verification ---------------------------------------
//
// `start` issues (or returns a still-valid) verification code. The
// user pastes it into their RSI public bio, then `verify` re-fetches
// the profile and looks for the code. Both endpoints take the user
// bearer; the desktop client doesn't surface bio editing — this
// flow is web-only.

export async function rsiVerifyStart(
  bearer: string,
): Promise<RsiStartResponse> {
  return postJson<RsiStartResponse>('/v1/auth/rsi/start', {}, bearer);
}

export async function rsiVerifyCheck(
  bearer: string,
): Promise<RsiVerifyResponse> {
  return postJson<RsiVerifyResponse>('/v1/auth/rsi/verify', {}, bearer);
}

// -- RSI citizen profile snapshot ----------------------------------
//
// Snapshot of the RSI public profile page (display name, enlistment
// date, badges, bio, primary org). The server caches the result —
// `refreshProfile` re-scrapes RSI (rate-limited to 429 if called
// too eagerly), `getMyProfile` returns the cached snapshot for the
// authenticated user, and `getPublicProfile` returns it for any
// public profile by handle (no auth).

export type ProfileResponse = apiSchema['schemas']['ProfileResponse'];
export type Badge = apiSchema['schemas']['Badge'];

export async function refreshProfile(bearer: string): Promise<ProfileResponse> {
  return postJson<ProfileResponse>('/v1/auth/rsi/profile/refresh', {}, bearer);
}

export async function getMyProfile(bearer: string): Promise<ProfileResponse> {
  return request<ProfileResponse>('GET', '/v1/me/profile', undefined, bearer);
}

/// Hangar snapshot — what the tray client most recently scraped from
/// the user's RSI website pledges page. The server stores the snapshot
/// in `hangar_snapshots`; the tray pushes via POST /v1/me/hangar; nothing
/// on the web actually wrote one here, but the dashboard + settings
/// pages now read it back so the user can see "yes, the tray fed us
/// 17 ships at 14:02" without launching the tray.
export type HangarSnapshot = apiSchema['schemas']['HangarSnapshot'];
export type HangarShip = apiSchema['schemas']['HangarShipSchema'];

/// 404 from the server means "no snapshot yet" — the user either
/// hasn't installed the tray, or hasn't paired it, or hasn't seeded
/// their RSI cookie. We surface that as a typed `null` rather than
/// asking every caller to try/catch a status code; matches the
/// `getCurrentLocation` pattern at `app/dashboard/page.tsx:74-81`.
export async function getMyHangar(
  bearer: string,
): Promise<HangarSnapshot | null> {
  try {
    return await request<HangarSnapshot>(
      'GET',
      '/v1/me/hangar',
      undefined,
      bearer,
    );
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 404) {
      return null;
    }
    throw e;
  }
}

export async function getPublicProfile(
  handle: string,
): Promise<ProfileResponse> {
  return request<ProfileResponse>(
    'GET',
    `/v1/public/u/${encodeURIComponent(handle)}/profile`,
    undefined,
    undefined,
  );
}

// -- RSI org snapshots ---------------------------------------------
//
// Triad mirrors the citizen-profile flow above:
//   * `refreshRsiOrgs` — owner pokes the server to scrape their
//     public RSI org page and persist a snapshot.
//   * `getMyRsiOrgs` — owner reads the most recent snapshot.
//   * `getPublicRsiOrgs` — anyone reads the snapshot for `handle`
//     if visibility allows (the server enforces public/share gating).
//
// All three return `RsiOrgsSnapshot { captured_at, orgs }`. 404 on
// the read endpoints means "no snapshot yet" / "not visible" — the
// callers convert that to a typed null using the same pattern as
// `getMyHangar`.

export type RsiOrgsSnapshot = apiSchema['schemas']['RsiOrgsSnapshot'];
export type RsiOrg = apiSchema['schemas']['RsiOrg'];

export async function refreshRsiOrgs(bearer: string): Promise<RsiOrgsSnapshot> {
  return postJson<RsiOrgsSnapshot>('/v1/auth/rsi/orgs/refresh', {}, bearer);
}

export async function getMyRsiOrgs(
  bearer: string,
): Promise<RsiOrgsSnapshot | null> {
  try {
    return await request<RsiOrgsSnapshot>(
      'GET',
      '/v1/me/rsi-orgs',
      undefined,
      bearer,
    );
  } catch (e) {
    if (e instanceof ApiCallError && e.status === 404) {
      return null;
    }
    throw e;
  }
}

export async function getPublicRsiOrgs(
  handle: string,
): Promise<RsiOrgsSnapshot | null> {
  try {
    return await request<RsiOrgsSnapshot>(
      'GET',
      `/v1/public/u/${encodeURIComponent(handle)}/orgs`,
      undefined,
      undefined,
    );
  } catch (e) {
    if (e instanceof ApiCallError && (e.status === 404 || e.status === 403)) {
      return null;
    }
    throw e;
  }
}

// -- Magic-link sign-in --------------------------------------------
//
// `start` is anti-enumeration: always returns 200 even on miss.
// `redeem` consumes the token and returns an `AuthResponse` —
// possibly with `totp_required: true` if the account has 2FA.

export async function magicLinkStart(
  body: MagicLinkStartRequest,
): Promise<MagicLinkStartResponse> {
  return postJson<MagicLinkStartResponse>('/v1/auth/magic/start', body);
}

export async function magicLinkRedeem(
  body: MagicLinkRedeemRequest,
): Promise<AuthResponse> {
  return postJson<AuthResponse>('/v1/auth/magic/redeem', body);
}

// -- TOTP 2FA ------------------------------------------------------
//
// Setup, confirm, disable, regenerate are authenticated with the
// regular user bearer. `verify-login` is the post-password leg of
// 2FA login: the bearer is the *interim* token returned by /login
// or /magic/redeem when `totp_required` was true.

export async function totpSetup(bearer: string): Promise<TotpSetupResponse> {
  return postJson<TotpSetupResponse>('/v1/auth/totp/setup', {}, bearer);
}

export async function totpConfirm(
  bearer: string,
  body: TotpConfirmRequest,
): Promise<TotpConfirmResponse> {
  return postJson<TotpConfirmResponse>('/v1/auth/totp/confirm', body, bearer);
}

export async function totpDisable(
  bearer: string,
  body: TotpDisableRequest,
): Promise<TotpDisableResponse> {
  return postJson<TotpDisableResponse>('/v1/auth/totp/disable', body, bearer);
}

export async function totpRegenerateRecovery(
  bearer: string,
  body: RegenerateRecoveryRequest,
): Promise<RegenerateRecoveryResponse> {
  return postJson<RegenerateRecoveryResponse>(
    '/v1/auth/totp/recovery/regenerate',
    body,
    bearer,
  );
}

export async function totpVerifyLogin(
  interimToken: string,
  body: VerifyLoginRequest,
): Promise<AuthResponse> {
  return postJson<AuthResponse>(
    '/v1/auth/totp/verify-login',
    body,
    interimToken,
  );
}

export async function getTimeline(
  bearer: string,
  params: { days?: number } = {},
): Promise<TimelineResponse> {
  const qs = new URLSearchParams();
  if (params.days !== undefined) qs.set('days', String(params.days));
  const suffix = qs.toString() ? `?${qs.toString()}` : '';
  return request<TimelineResponse>(
    'GET',
    `/v1/me/timeline${suffix}`,
    undefined,
    bearer,
  );
}

// -- Metrics aggregates ---------------------------------------------
//
// Powers the /metrics page (4 tabs). Overview reuses getSummary +
// getTimeline; the two helpers below cover the new aggregates.

export type EventTypeBreakdownResponse =
  apiSchema['schemas']['EventTypeBreakdownResponse'];
export type EventTypeStatsDto = apiSchema['schemas']['EventTypeStatsDto'];
export type SessionsResponse = apiSchema['schemas']['SessionsResponse'];
export type SessionDto = apiSchema['schemas']['SessionDto'];

export type MetricsRange = '7d' | '30d' | '90d' | 'all';

export async function getMetricsEventTypes(
  bearer: string,
  range: MetricsRange = '30d',
): Promise<EventTypeBreakdownResponse> {
  return request<EventTypeBreakdownResponse>(
    'GET',
    `/v1/me/metrics/event-types?range=${encodeURIComponent(range)}`,
    undefined,
    bearer,
  );
}

export async function getMetricsSessions(
  bearer: string,
  params: { limit?: number; offset?: number } = {},
): Promise<SessionsResponse> {
  const qs = new URLSearchParams();
  if (params.limit !== undefined) qs.set('limit', String(params.limit));
  if (params.offset !== undefined) qs.set('offset', String(params.offset));
  const suffix = qs.toString() ? `?${qs.toString()}` : '';
  return request<SessionsResponse>(
    'GET',
    `/v1/me/metrics/sessions${suffix}`,
    undefined,
    bearer,
  );
}

export type IngestHistoryResponse =
  apiSchema['schemas']['IngestHistoryResponse'];
export type IngestBatchDto = apiSchema['schemas']['IngestBatchDto'];

export async function getIngestHistory(
  bearer: string,
  params: { limit?: number; offset?: number; deviceId?: string } = {},
): Promise<IngestHistoryResponse> {
  const qs = new URLSearchParams();
  if (params.limit !== undefined) qs.set('limit', String(params.limit));
  if (params.offset !== undefined) qs.set('offset', String(params.offset));
  // When deviceId is passed the server clamps to only that device's
  // batches. Omitted → account-wide stream (current default).
  if (params.deviceId) qs.set('device_id', params.deviceId);
  const suffix = qs.toString() ? `?${qs.toString()}` : '';
  return request<IngestHistoryResponse>(
    'GET',
    `/v1/me/ingest-history${suffix}`,
    undefined,
    bearer,
  );
}

// -- Submissions ----------------------------------------------------
//
// Wraps the /v1/submissions endpoints. Voting + flagging are
// per-(user, submission) idempotent on the server side; the toggle
// behaviour comes from passing `vote: false` to retract.

export type SubmissionDto = apiSchema['schemas']['SubmissionDto'];
export type SubmissionListResponse = apiSchema['schemas']['ListResponse'];
export type CreateSubmissionRequest =
  apiSchema['schemas']['CreateSubmissionRequest'];
export type CreateSubmissionResponse =
  apiSchema['schemas']['CreateSubmissionResponse'];
export type VoteRequest = apiSchema['schemas']['VoteRequest'];
export type VoteResponse = apiSchema['schemas']['VoteResponse'];
export type FlagRequest = apiSchema['schemas']['FlagRequest'];
export type FlagResponse = apiSchema['schemas']['FlagResponse'];
export type WithdrawResponse = apiSchema['schemas']['WithdrawResponse'];

export type AdminQueueResponse =
  apiSchema['schemas']['AdminQueueResponse'];
export type AuditEntryDto = apiSchema['schemas']['AuditEntryDto'];
export type AuditListResponse = apiSchema['schemas']['AuditListResponse'];
export type AdminUserDto = apiSchema['schemas']['AdminUserDto'];
export type AdminUserListResponse =
  apiSchema['schemas']['AdminUserListResponse'];
export type GrantRoleRequest = apiSchema['schemas']['GrantRoleRequest'];
export type RoleTransitionResponse =
  apiSchema['schemas']['RoleTransitionResponse'];
export type AdminOrgDto = apiSchema['schemas']['AdminOrgDto'];
export type AdminOrgListResponse =
  apiSchema['schemas']['AdminOrgListResponse'];
export type AdminOrgDeleteResponse =
  apiSchema['schemas']['AdminOrgDeleteResponse'];
export type AdminReferenceCategoryDto =
  apiSchema['schemas']['AdminReferenceCategoryDto'];
export type AdminReferenceCategoriesResponse =
  apiSchema['schemas']['AdminReferenceCategoriesResponse'];
export type AdminReferenceEntryDto =
  apiSchema['schemas']['AdminReferenceEntryDto'];
export type AdminReferenceEntriesResponse =
  apiSchema['schemas']['AdminReferenceEntriesResponse'];
export type SubmissionTransitionResponse =
  apiSchema['schemas']['SubmissionTransitionResponse'];

export type SubmissionStatus =
  | 'review'
  | 'accepted'
  | 'shipped'
  | 'rejected'
  | 'withdrawn'
  | 'flagged';

export async function listSubmissions(
  bearer: string,
  params: {
    status?: SubmissionStatus;
    mine?: boolean;
    limit?: number;
    offset?: number;
  } = {},
): Promise<SubmissionListResponse> {
  const qs = new URLSearchParams();
  if (params.status) qs.set('status', params.status);
  if (params.mine) qs.set('mine', 'true');
  if (params.limit !== undefined) qs.set('limit', String(params.limit));
  if (params.offset !== undefined) qs.set('offset', String(params.offset));
  const suffix = qs.toString() ? `?${qs.toString()}` : '';
  return request<SubmissionListResponse>(
    'GET',
    `/v1/submissions${suffix}`,
    undefined,
    bearer,
  );
}

export async function getSubmission(
  bearer: string,
  id: string,
): Promise<SubmissionDto> {
  return request<SubmissionDto>(
    'GET',
    `/v1/submissions/${encodeURIComponent(id)}`,
    undefined,
    bearer,
  );
}

export async function createSubmission(
  bearer: string,
  body: CreateSubmissionRequest,
): Promise<CreateSubmissionResponse> {
  return request<CreateSubmissionResponse>(
    'POST',
    '/v1/submissions',
    body,
    bearer,
  );
}

export async function voteOnSubmission(
  bearer: string,
  id: string,
  vote: boolean,
): Promise<VoteResponse> {
  return request<VoteResponse>(
    'POST',
    `/v1/submissions/${encodeURIComponent(id)}/vote`,
    { vote },
    bearer,
  );
}

export async function flagSubmission(
  bearer: string,
  id: string,
  reason?: string,
): Promise<FlagResponse> {
  return request<FlagResponse>(
    'POST',
    `/v1/submissions/${encodeURIComponent(id)}/flag`,
    { reason: reason ?? null },
    bearer,
  );
}

export async function withdrawSubmission(
  bearer: string,
  id: string,
): Promise<WithdrawResponse> {
  return request<WithdrawResponse>(
    'POST',
    `/v1/submissions/${encodeURIComponent(id)}/withdraw`,
    undefined,
    bearer,
  );
}

// -- Admin (moderator + admin) -------------------------------------
//
// All four endpoints below require a staff role (moderator or admin)
// — server-side enforced via `StaffRoleSet::has`. The web client gates
// the /admin route surface on `session.staffRoles` for UX, but never
// trusts the cookie alone for authorization.

export async function getAdminSubmissionQueue(
  bearer: string,
  params: {
    status: 'review' | 'flagged' | 'all';
    limit?: number;
    offset?: number;
  },
): Promise<AdminQueueResponse> {
  const qs = new URLSearchParams();
  qs.set('status', params.status);
  if (params.limit !== undefined) qs.set('limit', String(params.limit));
  if (params.offset !== undefined) qs.set('offset', String(params.offset));
  return request<AdminQueueResponse>(
    'GET',
    `/v1/admin/submissions/queue?${qs.toString()}`,
    undefined,
    bearer,
  );
}

/** Paginated users list for /admin/users. Substring search runs
 *  server-side over claimed_handle OR email. */
export async function getAdminUsers(
  bearer: string,
  params: { q?: string; limit?: number; offset?: number } = {},
): Promise<AdminUserListResponse> {
  const qs = new URLSearchParams();
  if (params.q) qs.set('q', params.q);
  if (params.limit !== undefined) qs.set('limit', String(params.limit));
  if (params.offset !== undefined) qs.set('offset', String(params.offset));
  const suffix = qs.toString() ? `?${qs.toString()}` : '';
  return request<AdminUserListResponse>(
    'GET',
    `/v1/admin/users${suffix}`,
    undefined,
    bearer,
  );
}

/** Detail fetch for a single user in /admin/users/[id]. */
export async function getAdminUser(
  bearer: string,
  id: string,
): Promise<AdminUserDto> {
  return request<AdminUserDto>(
    'GET',
    `/v1/admin/users/${encodeURIComponent(id)}`,
    undefined,
    bearer,
  );
}

/** Grant a staff role to a user. Admin-only. Idempotent. */
export async function grantAdminUserRole(
  bearer: string,
  id: string,
  body: GrantRoleRequest,
): Promise<RoleTransitionResponse> {
  return postJson<RoleTransitionResponse>(
    `/v1/admin/users/${encodeURIComponent(id)}/roles`,
    body,
    bearer,
  );
}

/** Revoke a staff role from a user. Admin-only. Idempotent. */
export async function revokeAdminUserRole(
  bearer: string,
  id: string,
  role: 'moderator' | 'admin',
): Promise<RoleTransitionResponse> {
  return request<RoleTransitionResponse>(
    'DELETE',
    `/v1/admin/users/${encodeURIComponent(id)}/roles/${encodeURIComponent(role)}`,
    undefined,
    bearer,
  );
}

/** Paginated orgs list (admin view across ALL orgs). */
export async function getAdminOrgs(
  bearer: string,
  params: { q?: string; limit?: number; offset?: number } = {},
): Promise<AdminOrgListResponse> {
  const qs = new URLSearchParams();
  if (params.q) qs.set('q', params.q);
  if (params.limit !== undefined) qs.set('limit', String(params.limit));
  if (params.offset !== undefined) qs.set('offset', String(params.offset));
  const suffix = qs.toString() ? `?${qs.toString()}` : '';
  return request<AdminOrgListResponse>(
    'GET',
    `/v1/admin/orgs${suffix}`,
    undefined,
    bearer,
  );
}

/** Org detail for the admin console. */
export async function getAdminOrg(
  bearer: string,
  slug: string,
): Promise<AdminOrgDto> {
  return request<AdminOrgDto>(
    'GET',
    `/v1/admin/orgs/${encodeURIComponent(slug)}`,
    undefined,
    bearer,
  );
}

/** Admin force-delete an org. Wipes SpiceDB relationships + the
 *  Postgres row. Admin-only. */
export async function deleteAdminOrg(
  bearer: string,
  slug: string,
): Promise<AdminOrgDeleteResponse> {
  return request<AdminOrgDeleteResponse>(
    'DELETE',
    `/v1/admin/orgs/${encodeURIComponent(slug)}`,
    undefined,
    bearer,
  );
}

/** Per-category summary of the reference_registry. Surfaces row count
 *  + last sync time so admins can spot a stuck cron at a glance. */
export async function getAdminReferenceCategories(
  bearer: string,
): Promise<AdminReferenceCategoriesResponse> {
  return request<AdminReferenceCategoriesResponse>(
    'GET',
    '/v1/admin/reference/categories',
    undefined,
    bearer,
  );
}

/** Paginated entry list for a single category. `q` is a
 *  case-insensitive substring filter on class_name + display_name. */
export async function getAdminReferenceEntries(
  bearer: string,
  category: string,
  params: { q?: string; limit?: number; offset?: number } = {},
): Promise<AdminReferenceEntriesResponse> {
  const qs = new URLSearchParams();
  if (params.q) qs.set('q', params.q);
  if (params.limit !== undefined) qs.set('limit', String(params.limit));
  if (params.offset !== undefined) qs.set('offset', String(params.offset));
  const suffix = qs.toString() ? `?${qs.toString()}` : '';
  return request<AdminReferenceEntriesResponse>(
    'GET',
    `/v1/admin/reference/${encodeURIComponent(category)}${suffix}`,
    undefined,
    bearer,
  );
}

/**
 * Paginated audit-log fetch for the /admin/audit page. Server is
 * gated on moderator role; the client gates the page on
 * `session.staffRoles` for UX but never trusts the cookie alone.
 *
 * Filters are passed through as querystring params; empty/undefined
 * filters are omitted so the server treats them as "no filter"
 * rather than "filter for empty string".
 */
export async function getAdminAuditLog(
  bearer: string,
  params: {
    actor?: string;
    action?: string;
    since?: string;
    until?: string;
    limit?: number;
    offset?: number;
  } = {},
): Promise<AuditListResponse> {
  const qs = new URLSearchParams();
  if (params.actor) qs.set('actor', params.actor);
  if (params.action) qs.set('action', params.action);
  if (params.since) qs.set('since', params.since);
  if (params.until) qs.set('until', params.until);
  if (params.limit !== undefined) qs.set('limit', String(params.limit));
  if (params.offset !== undefined) qs.set('offset', String(params.offset));
  const suffix = qs.toString() ? `?${qs.toString()}` : '';
  return request<AuditListResponse>(
    'GET',
    `/v1/admin/audit${suffix}`,
    undefined,
    bearer,
  );
}

export async function acceptSubmission(
  bearer: string,
  id: string,
): Promise<SubmissionTransitionResponse> {
  return request<SubmissionTransitionResponse>(
    'POST',
    `/v1/admin/submissions/${encodeURIComponent(id)}/accept`,
    undefined,
    bearer,
  );
}

export async function rejectSubmission(
  bearer: string,
  id: string,
  reason: string,
): Promise<SubmissionTransitionResponse> {
  return request<SubmissionTransitionResponse>(
    'POST',
    `/v1/admin/submissions/${encodeURIComponent(id)}/reject`,
    { reason },
    bearer,
  );
}

export async function dismissSubmissionFlag(
  bearer: string,
  id: string,
): Promise<SubmissionTransitionResponse> {
  return request<SubmissionTransitionResponse>(
    'POST',
    `/v1/admin/submissions/${encodeURIComponent(id)}/dismiss-flag`,
    undefined,
    bearer,
  );
}

// -- Supporter (donate) --------------------------------------------
//
// Read-only for now. The actual checkout / webhook flow depends on
// Revolut Business credentials being provisioned (see
// docs/REVOLUT-INTEGRATION-PLAN.md). The read endpoint already exists
// so the supporter pill on the profile / settings pages can light up
// against any manually-set row.

export type SupporterStatusDto =
  apiSchema['schemas']['SupporterStatusDto'];

export type SupporterState = 'none' | 'active' | 'lapsed';

export async function getSupporterStatus(
  bearer: string,
): Promise<SupporterStatusDto> {
  return request<SupporterStatusDto>(
    'GET',
    '/v1/me/supporter',
    undefined,
    bearer,
  );
}

// -- Location: where the user currently is in-game ---------------
//
// Backed by `GET /v1/me/location/current` on the server. Returns 204
// (translated to `null` here) when the most recent location-bearing
// event is older than the staleness window (90 minutes) — the UI
// uses null as the "no recent activity" signal.

export type ResolvedLocation = apiSchema['schemas']['ResolvedLocation'];
export type CurrentLocationResponse =
  apiSchema['schemas']['CurrentLocationResponse'];

export async function getCurrentLocation(
  bearer: string,
): Promise<ResolvedLocation | null> {
  // request<T>() already returns undefined on 204; we narrow to null
  // here so callers don't accidentally read fields off undefined.
  const resp = (await request<CurrentLocationResponse | undefined>(
    'GET',
    '/v1/me/location/current',
    undefined,
    bearer,
  )) as CurrentLocationResponse | undefined;
  return resp?.location ?? null;
}

export type TraceResponse = apiSchema['schemas']['TraceResponse'];
export type TraceEntry = apiSchema['schemas']['TraceEntry'];
export type BreakdownResponse = apiSchema['schemas']['BreakdownResponse'];
export type BreakdownEntry = apiSchema['schemas']['BreakdownEntry'];
export type StatsBucket = apiSchema['schemas']['StatsBucket'];
export type CombatStatsResponse =
  apiSchema['schemas']['CombatStatsResponse'];
export type TravelStatsResponse =
  apiSchema['schemas']['TravelStatsResponse'];
export type LoadoutStatsResponse =
  apiSchema['schemas']['LoadoutStatsResponse'];
export type StabilityStatsResponse =
  apiSchema['schemas']['StabilityStatsResponse'];

export async function getLocationTrace(
  bearer: string,
  hours: number = 24,
): Promise<TraceResponse> {
  return request<TraceResponse>(
    'GET',
    `/v1/me/location/trace?hours=${hours}`,
    undefined,
    bearer,
  );
}

export async function getLocationBreakdown(
  bearer: string,
  hours: number = 24 * 7,
): Promise<BreakdownResponse> {
  return request<BreakdownResponse>(
    'GET',
    `/v1/me/location/breakdown?hours=${hours}`,
    undefined,
    bearer,
  );
}

export async function getCombatStats(
  bearer: string,
  hours: number = 24 * 30,
): Promise<CombatStatsResponse> {
  return request<CombatStatsResponse>(
    'GET',
    `/v1/me/stats/combat?hours=${hours}`,
    undefined,
    bearer,
  );
}

export async function getTravelStats(
  bearer: string,
  hours: number = 24 * 30,
): Promise<TravelStatsResponse> {
  return request<TravelStatsResponse>(
    'GET',
    `/v1/me/stats/travel?hours=${hours}`,
    undefined,
    bearer,
  );
}

export async function getLoadoutStats(
  bearer: string,
  hours: number = 24 * 30,
): Promise<LoadoutStatsResponse> {
  return request<LoadoutStatsResponse>(
    'GET',
    `/v1/me/stats/loadout?hours=${hours}`,
    undefined,
    bearer,
  );
}

export type CommerceTxKind = 'shop' | 'commodity_buy' | 'commodity_sell';
export type CommerceTxStatus =
  | 'pending'
  | 'confirmed'
  | 'rejected'
  | 'timed_out'
  | 'submitted';

// Drift fix #5: switch the field shapes to come from the generated
// schema (server now registers CommerceTransactionDto +
// CommerceRecentResponse in openapi.rs). The two `kind` / `status`
// fields stay re-typed to the local literal unions because the
// server returns plain `String` — narrowing on the client side
// preserves call-site exhaustiveness checks (e.g.
// `formatCommerceStatus` in journey/page.tsx). Trade-off: a new
// kind/status variant added on the server will silently fall outside
// the union here until this file is updated. Long-term fix is to
// turn the Rust types into enums; until then, this comment + the
// narrowing is the contract.
export type CommerceTransaction = Omit<
  apiSchema['schemas']['CommerceTransactionDto'],
  'kind' | 'status'
> & {
  kind: CommerceTxKind;
  status: CommerceTxStatus;
};
// `CommerceRecentResponse` mirrors the server schema but with the
// inner array re-typed to the narrowed `CommerceTransaction` so the
// kind/status unions reach call sites.
export interface CommerceRecentResponse {
  transactions: CommerceTransaction[];
}

export async function getCommerceRecent(
  bearer: string,
  limit: number = 100,
  windowSecs: number = 30,
  /** Optional time-range filter in hours. When omitted the server
   *  pulls the last ~1000 events of any type and filters to commerce
   *  variants in-process (legacy behavior). When set, only events
   *  newer than `now - hours` are considered. Matches the journey
   *  range chip selector. */
  hours?: number,
): Promise<CommerceRecentResponse> {
  const params = new URLSearchParams({
    limit: String(limit),
    window_secs: String(windowSecs),
  });
  if (hours !== undefined) {
    params.set('hours', String(hours));
  }
  return request<CommerceRecentResponse>(
    'GET',
    `/v1/me/commerce/recent?${params.toString()}`,
    undefined,
    bearer,
  );
}

export async function getStabilityStats(
  bearer: string,
  hours: number = 24 * 30,
): Promise<StabilityStatsResponse> {
  return request<StabilityStatsResponse>(
    'GET',
    `/v1/me/stats/stability?hours=${hours}`,
    undefined,
    bearer,
  );
}

// -- Donate (Revolut Business hosted checkout) --------------------
//
// Wave 9. The server returns 503 `not_configured` when REVOLUT_API_KEY
// is unset, so the donate page renders a "coming soon" panel rather
// than a checkout button in that environment. The tier list is static
// (server-side const) but we fetch it through the API so future
// price-list edits don't require a frontend rebuild.

export type TierDto = apiSchema['schemas']['TierDto'];
export type TierListResponse = apiSchema['schemas']['TierListResponse'];
export type CheckoutRequest = apiSchema['schemas']['CheckoutRequest'];
export type CheckoutResponse = apiSchema['schemas']['CheckoutResponse'];

export async function listDonateTiers(): Promise<TierListResponse> {
  return request<TierListResponse>(
    'GET',
    '/v1/donate/tiers',
    undefined,
    undefined,
  );
}

export async function startDonateCheckout(
  bearer: string,
  body: CheckoutRequest,
): Promise<CheckoutResponse> {
  return request<CheckoutResponse>('POST', '/v1/donate/checkout', body, bearer);
}

// -- Sharing + visibility -------------------------------------------
//
// Server endpoints live in `crates/starstats-server/src/sharing_routes.rs`.
// Helpers here are thin wrappers that surface the generated schema
// types. The public read endpoints (`/v1/public/*`) are unauthenticated;
// the friend read endpoints (`/v1/u/*`) take a bearer.

export type VisibilityRequest = apiSchema['schemas']['VisibilityRequest'];
export type VisibilityResponse = apiSchema['schemas']['VisibilityResponse'];
export type ShareRequest = apiSchema['schemas']['ShareRequest'];
export type ShareResponse = apiSchema['schemas']['ShareResponse'];
export type RevokeShareResponse =
  apiSchema['schemas']['RevokeShareResponse'];
export type ListSharesResponse = apiSchema['schemas']['ListSharesResponse'];
export type ShareEntry = apiSchema['schemas']['ShareEntry'];
/**
 * Per-share scope clamp — wire-level shape generated from
 * `sharing_routes::ShareScope`. `null` (or omitted) means "full
 * manifest", the legacy default every pre-W3 share already has.
 */
export type ShareScope = apiSchema['schemas']['ShareScope'];
export type ListSharedWithMeResponse =
  apiSchema['schemas']['ListSharedWithMeResponse'];
export type SharedWithMeEntry =
  apiSchema['schemas']['SharedWithMeEntry'];
export type PublicSummaryResponse =
  apiSchema['schemas']['PublicSummaryResponse'];
export type PublicTimelineResponse =
  apiSchema['schemas']['PublicTimelineResponse'];

export async function getVisibility(
  bearer: string,
): Promise<VisibilityResponse> {
  return request<VisibilityResponse>(
    'GET',
    '/v1/me/visibility',
    undefined,
    bearer,
  );
}

export async function setVisibility(
  bearer: string,
  isPublic: boolean,
): Promise<VisibilityResponse> {
  return postJson<VisibilityResponse>(
    '/v1/me/visibility',
    { public: isPublic } satisfies VisibilityRequest,
    bearer,
  );
}

export async function listShares(bearer: string): Promise<ListSharesResponse> {
  return request<ListSharesResponse>(
    'GET',
    '/v1/me/shares',
    undefined,
    bearer,
  );
}

/**
 * Inbound side of per-user sharing: the owners who have granted
 * the caller view access to their stats_record. Mirrors
 * `listShares` (outbound) but on the receiving end. Org-mediated
 * shares aren't enumerated here — those come from /v1/orgs/me +
 * the per-org detail page.
 */
export async function listSharedWithMe(
  bearer: string,
): Promise<ListSharedWithMeResponse> {
  return request<ListSharedWithMeResponse>(
    'GET',
    '/v1/me/shared-with-me',
    undefined,
    bearer,
  );
}

export async function addShare(
  bearer: string,
  recipientHandle: string,
  options: {
    expiresAt?: string | null;
    note?: string | null;
    /**
     * Per-share scope clamp. `null` or omitted is the legacy
     * "full manifest" default. The server normalises `kind="full"`
     * back to NULL so re-grants from a UI that always sends a scope
     * can still clear it.
     */
    scope?: ShareScope | null;
  } = {},
): Promise<ShareResponse> {
  const body: ShareRequest = {
    recipient_handle: recipientHandle,
  };
  // Only include the optional fields when set so the server doesn't
  // see explicit nulls — the Rust handler treats absence and null
  // the same way, but absence is the canonical "no expiry / no note"
  // shape that round-trips cleanly with #[serde(default)].
  if (options.expiresAt) body.expires_at = options.expiresAt;
  if (options.note) body.note = options.note;
  if (options.scope) body.scope = options.scope;
  return postJson<ShareResponse>('/v1/me/share', body, bearer);
}

export async function removeShare(
  bearer: string,
  recipientHandle: string,
): Promise<RevokeShareResponse> {
  return request<RevokeShareResponse>(
    'DELETE',
    `/v1/me/share/${encodeURIComponent(recipientHandle)}`,
    undefined,
    bearer,
  );
}

export async function getPublicSummary(
  handle: string,
): Promise<PublicSummaryResponse> {
  return request<PublicSummaryResponse>(
    'GET',
    `/v1/public/${encodeURIComponent(handle)}/summary`,
    undefined,
    undefined,
  );
}

export async function getPublicTimeline(
  handle: string,
  days?: number,
): Promise<PublicTimelineResponse> {
  const qs = new URLSearchParams();
  if (days !== undefined) qs.set('days', String(days));
  const suffix = qs.toString() ? `?${qs.toString()}` : '';
  return request<PublicTimelineResponse>(
    'GET',
    `/v1/public/${encodeURIComponent(handle)}/timeline${suffix}`,
    undefined,
    undefined,
  );
}

export async function getFriendSummary(
  bearer: string,
  handle: string,
): Promise<PublicSummaryResponse> {
  return request<PublicSummaryResponse>(
    'GET',
    `/v1/u/${encodeURIComponent(handle)}/summary`,
    undefined,
    bearer,
  );
}

export async function getFriendTimeline(
  bearer: string,
  handle: string,
  days?: number,
): Promise<PublicTimelineResponse> {
  const qs = new URLSearchParams();
  if (days !== undefined) qs.set('days', String(days));
  const suffix = qs.toString() ? `?${qs.toString()}` : '';
  return request<PublicTimelineResponse>(
    'GET',
    `/v1/u/${encodeURIComponent(handle)}/timeline${suffix}`,
    undefined,
    bearer,
  );
}

// -- Organizations + org-share -------------------------------------
//
// Server endpoints live in `crates/starstats-server/src/org_routes.rs`
// and the org-share half of `sharing_routes.rs`. The slug is
// generated server-side; clients only ever pass a display name on
// create.

export type OrgDto = apiSchema['schemas']['OrgDto'];
export type OrgMemberDto = apiSchema['schemas']['OrgMemberDto'];
export type CreateOrgRequest = apiSchema['schemas']['CreateOrgRequest'];
export type CreateOrgResponse = apiSchema['schemas']['CreateOrgResponse'];
export type ListOrgsResponse = apiSchema['schemas']['ListOrgsResponse'];
export type GetOrgResponse = apiSchema['schemas']['GetOrgResponse'];
export type DeleteOrgResponse = apiSchema['schemas']['DeleteOrgResponse'];
export type AddMemberRequest = apiSchema['schemas']['AddMemberRequest'];
export type AddMemberResponse = apiSchema['schemas']['AddMemberResponse'];
export type RemoveMemberResponse =
  apiSchema['schemas']['RemoveMemberResponse'];
export type OrgShareEntry = apiSchema['schemas']['OrgShareEntry'];
export type ShareOrgRequest = apiSchema['schemas']['ShareOrgRequest'];
export type ShareOrgResponse = apiSchema['schemas']['ShareOrgResponse'];
export type RevokeOrgShareResponse =
  apiSchema['schemas']['RevokeOrgShareResponse'];

export async function createOrg(
  bearer: string,
  body: { name: string },
): Promise<CreateOrgResponse> {
  return postJson<CreateOrgResponse>(
    '/v1/orgs',
    { name: body.name } satisfies CreateOrgRequest,
    bearer,
  );
}

export async function listOrgs(bearer: string): Promise<ListOrgsResponse> {
  return request<ListOrgsResponse>('GET', '/v1/orgs', undefined, bearer);
}

export async function getOrg(
  bearer: string,
  slug: string,
): Promise<GetOrgResponse> {
  return request<GetOrgResponse>(
    'GET',
    `/v1/orgs/${encodeURIComponent(slug)}`,
    undefined,
    bearer,
  );
}

export async function deleteOrg(
  bearer: string,
  slug: string,
): Promise<DeleteOrgResponse> {
  return request<DeleteOrgResponse>(
    'DELETE',
    `/v1/orgs/${encodeURIComponent(slug)}`,
    undefined,
    bearer,
  );
}

export async function addOrgMember(
  bearer: string,
  slug: string,
  body: { handle: string; role: 'admin' | 'member' },
): Promise<AddMemberResponse> {
  return postJson<AddMemberResponse>(
    `/v1/orgs/${encodeURIComponent(slug)}/members`,
    { handle: body.handle, role: body.role } satisfies AddMemberRequest,
    bearer,
  );
}

export async function removeOrgMember(
  bearer: string,
  slug: string,
  handle: string,
): Promise<RemoveMemberResponse> {
  return request<RemoveMemberResponse>(
    'DELETE',
    `/v1/orgs/${encodeURIComponent(slug)}/members/${encodeURIComponent(handle)}`,
    undefined,
    bearer,
  );
}

export async function shareWithOrg(
  bearer: string,
  slug: string,
): Promise<ShareOrgResponse> {
  return postJson<ShareOrgResponse>(
    '/v1/me/share/org',
    { org_slug: slug } satisfies ShareOrgRequest,
    bearer,
  );
}

export async function unshareWithOrg(
  bearer: string,
  slug: string,
): Promise<RevokeOrgShareResponse> {
  return request<RevokeOrgShareResponse>(
    'DELETE',
    `/v1/me/share/org/${encodeURIComponent(slug)}`,
    undefined,
    bearer,
  );
}

// -- User preferences ----------------------------------------------
//
// Aliased to the generated schema's `UserPreferencesSchema`. Drift
// fix #5: the codegen has had this type for a while; lib/api.ts was
// just lagging behind its own TODO.
export type UserPreferences = apiSchema['schemas']['UserPreferencesSchema'];

export async function getPreferences(
  bearer: string,
): Promise<UserPreferences> {
  return request<UserPreferences>(
    'GET',
    '/v1/me/preferences',
    undefined,
    bearer,
  );
}

export async function putPreferences(
  bearer: string,
  prefs: UserPreferences,
): Promise<void> {
  await putJson<void>('/v1/me/preferences', prefs, bearer);
}

// Reference data
export type VehicleReference = apiSchema['schemas']['VehicleReference'];
export type VehicleListResponse = apiSchema['schemas']['VehicleListResponse'];

/**
 * Fetch the vehicle reference catalogue. Unauthenticated; the server
 * caches it from the Star Citizen Wiki API on a 24h cron. Used for
 * mapping raw class names like `CRUS_Starfighter_Ion` to display names
 * on the dashboard timeline.
 *
 * Cached at the Next.js fetch layer for an hour so re-renders of the
 * dashboard (which fetch the catalogue on every server render) don't
 * pull ~150 rows over the wire each time. The upstream refreshes once
 * per 24h server-side, so an hour of staleness is invisible.
 */
export async function getVehicleReferences(): Promise<VehicleListResponse> {
  const resp = await fetch(`${apiBase()}/v1/reference/vehicles`, {
    method: 'GET',
    next: { revalidate: 3600 },
  });
  if (!resp.ok) {
    let errBody: ApiError;
    try {
      errBody = (await resp.json()) as ApiError;
    } catch {
      errBody = { error: `http_${resp.status}` };
    }
    throw new ApiCallError(resp.status, errBody);
  }
  return (await resp.json()) as VehicleListResponse;
}

// -- Admin: SMTP config ---------------------------------------------

/** Fetch current SMTP config (password redacted; presence flag is on
 *  the response as `password_set`). 403 if caller isn't an admin. */
export async function getSmtpConfig(
  bearer: string,
): Promise<SmtpConfigResponse> {
  return request<SmtpConfigResponse>('GET', '/v1/admin/smtp', undefined, bearer);
}

/** Persist a new SMTP config + hot-swap the runtime mailer. The
 *  `password` field on the body tri-state: omit (null) = keep
 *  existing, "" = clear auth, non-empty = set new. Returns the
 *  re-read row so the form can refresh state from the server. */
export async function putSmtpConfig(
  body: SmtpConfigRequest,
  bearer: string,
): Promise<SmtpConfigResponse> {
  return putJson<SmtpConfigResponse>('/v1/admin/smtp', body, bearer);
}

/** Trigger a diagnostic email to the calling admin's verified
 *  address. 400 if email is unverified, 502 if the SMTP send fails. */
export async function testSmtp(
  bearer: string,
  toAddress?: string | null,
): Promise<TestSendResponse> {
  const body =
    toAddress && toAddress.trim().length > 0
      ? { to_address: toAddress.trim() }
      : {};
  return postJson<TestSendResponse>('/v1/admin/smtp/test', body, bearer);
}
