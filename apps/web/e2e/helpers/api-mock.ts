/**
 * Helpers for talking to the local mock API server.
 *
 * Every test posts a fresh "scenario" to the mock before navigating.
 * The scenario is just a `Record<"METHOD path", ResponseStub>` — keys
 * are matched exactly against the incoming Node fetch from the Next
 * dev server.
 *
 * Most fixtures here are intentionally small and synthetic; they
 * mirror the OpenAPI shapes from `packages/api-client-ts/src/generated/schema.ts`
 * but don't try to be exhaustive. Each test extends/overrides only
 * what it cares about.
 */

import type { APIRequestContext, Page } from '@playwright/test';

export const MOCK_PORT = 3199;
export const WEB_PORT = 3100;
export const MOCK_BASE = `http://127.0.0.1:${MOCK_PORT}`;

export interface ResponseStub {
  status?: number;
  body?: unknown;
}

export type ScenarioRoutes = Record<string, ResponseStub>;

export interface Scenario {
  __id?: string;
  routes: ScenarioRoutes;
}

/**
 * POST a scenario to the mock server. Call this in `test.beforeEach`
 * (or inline before `page.goto`) so the mock answers with the shapes
 * the assertions expect.
 */
export async function setScenario(
  request: APIRequestContext,
  scenario: Scenario,
): Promise<void> {
  const resp = await request.post(`${MOCK_BASE}/__mock/scenario`, {
    data: scenario,
  });
  if (!resp.ok()) {
    throw new Error(`setScenario failed: ${resp.status()} ${await resp.text()}`);
  }
}

export async function resetScenario(request: APIRequestContext): Promise<void> {
  await request.post(`${MOCK_BASE}/__mock/reset`);
}

/**
 * Read the call log from the mock. Useful for asserting the dashboard
 * issued a request with the right query string, etc.
 */
export async function getCalls(
  request: APIRequestContext,
): Promise<
  Array<{ method: string; path: string; query: string; body: unknown }>
> {
  const resp = await request.get(`${MOCK_BASE}/__mock/calls`);
  const data = (await resp.json()) as {
    calls: Array<{ method: string; path: string; query: string; body: unknown }>;
  };
  return data.calls;
}

/**
 * Set the session cookie directly so the test starts "logged in"
 * without having to walk through the auth flow. Mirrors the cookie
 * shape minted by `setSession()` in `src/lib/session.ts`.
 */
export async function loginAs(
  page: Page,
  opts: {
    token?: string;
    userId?: string;
    handle?: string;
    emailVerified?: boolean;
    /// Site-wide staff grants. Empty by default — tests opt in to
    /// admin / moderator routes by passing `['admin']` or
    /// `['moderator']`. Mirrors `staffRoles` in the Session type.
    staffRoles?: string[];
  } = {},
): Promise<void> {
  const value = JSON.stringify({
    t: opts.token ?? 'test-token',
    u: opts.userId ?? 'user_test',
    h: opts.handle ?? 'TestPilot',
    v: opts.emailVerified ?? true,
    r: opts.staffRoles ?? [],
  });
  await page.context().addCookies([
    {
      name: 'starstats_session',
      value,
      domain: 'localhost',
      path: '/',
      httpOnly: true,
      sameSite: 'Lax',
    },
  ]);
}

// ---------------------------------------------------------------------
// Fixtures — concrete shapes that match the OpenAPI components.
// ---------------------------------------------------------------------

export const successfulSignup = {
  status: 200,
  body: {
    token: 'jwt.signup.token',
    user_id: 'user_new',
    claimed_handle: 'TestPilot',
  },
};

export const successfulLogin = {
  status: 200,
  body: {
    token: 'jwt.login.token',
    user_id: 'user_existing',
    claimed_handle: 'TestPilot',
  },
};

export const currentUser = {
  status: 200,
  body: {
    user_id: 'user_existing',
    email: 'pilot@example.test',
    email_verified: true,
    claimed_handle: 'TestPilot',
  },
};

export const currentUserUnverified = {
  status: 200,
  body: {
    ...currentUser.body,
    email_verified: false,
  },
};

export const summaryWithEvents = {
  status: 200,
  body: {
    claimed_handle: 'TestPilot',
    total: 1234,
    by_type: [
      { event_type: 'login', count: 600 },
      { event_type: 'mission_complete', count: 400 },
      { event_type: 'death', count: 234 },
    ],
  },
};

export const emptySummary = {
  status: 200,
  body: {
    claimed_handle: 'TestPilot',
    total: 0,
    by_type: [],
  },
};

export const timeline30Days = {
  status: 200,
  body: {
    days: 30,
    buckets: Array.from({ length: 30 }, (_, i) => ({
      date: `2026-04-${String(i + 1).padStart(2, '0')}`,
      count: i % 5 === 0 ? 0 : (i + 1) * 3,
    })),
  },
};

/**
 * 50-event page so the dashboard's pager renders an "Older →" link.
 */
export const eventsPageDescending = {
  status: 200,
  body: {
    events: Array.from({ length: 50 }, (_, i) => {
      const seq = 100 - i;
      return {
        seq,
        source_offset: seq * 1024,
        log_source: 'live',
        event_type: i % 2 === 0 ? 'login' : 'mission_complete',
        event_timestamp: '2026-05-04T12:00:00Z',
        payload: { type: i % 2 === 0 ? 'login' : 'mission_complete' },
      };
    }),
    next_after: null,
  },
};

export const eventsFilteredLogin = {
  status: 200,
  body: {
    events: [
      {
        seq: 100,
        source_offset: 102400,
        log_source: 'live',
        event_type: 'login',
        event_timestamp: '2026-05-04T12:00:00Z',
        payload: { type: 'login' },
      },
      {
        seq: 98,
        source_offset: 100352,
        log_source: 'live',
        event_type: 'login',
        event_timestamp: '2026-05-04T11:30:00Z',
        payload: { type: 'login' },
      },
    ],
    next_after: null,
  },
};

export const deviceList = {
  status: 200,
  body: {
    devices: [
      {
        id: 'dev_1',
        label: "Daisy's PC",
        created_at: '2026-04-01T08:00:00Z',
        last_seen_at: '2026-05-04T07:00:00Z',
      },
    ],
  },
};

export const visibilityPrivate = {
  status: 200,
  body: { public: false },
};

export const noShares = {
  status: 200,
  body: { shares: [], org_shares: [] },
};

export const noOrgs = {
  status: 200,
  body: { orgs: [] },
};

export const ownedOrgs = {
  status: 200,
  body: {
    orgs: [
      {
        id: 'org_1',
        name: 'Test Squadron',
        slug: 'test-squadron',
        owner_user_id: 'user_existing',
        created_at: '2026-01-01T00:00:00Z',
      },
      {
        id: 'org_2',
        name: 'Aegis Pilots',
        slug: 'aegis-pilots',
        owner_user_id: 'user_existing',
        created_at: '2026-02-01T00:00:00Z',
      },
    ],
  },
};

export function orgDetail(slug: string, name: string) {
  return {
    status: 200,
    body: {
      org: {
        id: `org_${slug}`,
        name,
        slug,
        owner_user_id: 'user_existing',
        created_at: '2026-01-01T00:00:00Z',
      },
      members: [
        { handle: 'TestPilot', role: 'owner' },
        { handle: 'WingmanOne', role: 'admin' },
      ],
      your_role: 'owner',
    },
  };
}

export const publicSummaryShared = {
  status: 200,
  body: {
    claimed_handle: 'JohnSomeone',
    total: 42,
    by_type: [
      { event_type: 'login', count: 30 },
      { event_type: 'death', count: 12 },
    ],
  },
};

export const notFound = {
  status: 404,
  body: { error: 'not_found' },
};

export const unauthorized = {
  status: 401,
  body: { error: 'invalid_credentials' },
};

export const conflict = (code: string) => ({
  status: 409,
  body: { error: code },
});

/**
 * Compose a scenario from the most common "logged in user with data
 * everywhere" defaults plus per-test overrides. Override keys take
 * precedence over the defaults.
 */
export function scenarioFor(
  id: string,
  overrides: ScenarioRoutes = {},
): Scenario {
  const base: ScenarioRoutes = {
    'GET /v1/auth/me': currentUser,
    'GET /v1/me/summary': summaryWithEvents,
    'GET /v1/me/events': eventsPageDescending,
    'GET /v1/me/timeline': timeline30Days,
    'GET /v1/auth/devices': deviceList,
    'GET /v1/me/visibility': visibilityPrivate,
    'GET /v1/me/shares': noShares,
    'GET /v1/orgs': noOrgs,
  };
  return { __id: id, routes: { ...base, ...overrides } };
}
