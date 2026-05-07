import { expect, test } from '@playwright/test';
import {
  eventsFilteredLogin,
  eventsPageDescending,
  getCalls,
  loginAs,
  resetScenario,
  scenarioFor,
  setScenario,
  summaryWithEvents,
  timeline30Days,
} from './helpers/api-mock';

test.beforeEach(async ({ request, page }) => {
  await resetScenario(request);
  await loginAs(page);
});

test('dashboard_renders_top_types_and_timeline', async ({ page, request }) => {
  await setScenario(
    request,
    scenarioFor('dashboard_default', {
      'GET /v1/me/summary': summaryWithEvents,
      'GET /v1/me/events': eventsPageDescending,
      'GET /v1/me/timeline': timeline30Days,
    }),
  );

  await page.goto('/dashboard');

  await expect(page.getByRole('heading', { name: /Hi, TestPilot/ })).toBeVisible();
  await expect(page.getByText('1,234 events captured')).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Top event types' })).toBeVisible();
  // Top-types card renders each event type as a `<Link className="mono">`.
  await expect(page.locator('a.mono').filter({ hasText: 'login' }).first()).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Last 30 days' })).toBeVisible();
  // 30-day chart is now a CSS-grid heatmap with role="img".
  await expect(page.getByRole('img', { name: /Per-day event counts/ })).toBeVisible();
});

test('dashboard_clicking_event_type_drills_down', async ({ page, request }) => {
  await setScenario(
    request,
    scenarioFor('dashboard_drilldown', {
      'GET /v1/me/summary': summaryWithEvents,
      'GET /v1/me/events': eventsPageDescending,
      'GET /v1/me/timeline': timeline30Days,
    }),
  );
  await page.goto('/dashboard');

  // Re-arm scenario for the filtered re-fetch.
  await setScenario(
    request,
    scenarioFor('dashboard_drilldown_filtered', {
      'GET /v1/me/summary': summaryWithEvents,
      'GET /v1/me/events': eventsFilteredLogin,
      'GET /v1/me/timeline': timeline30Days,
    }),
  );

  // Top-types card has the type code wrapped in a <Link className="mono">;
  // clicking the link navigates to /dashboard?type=login.
  await page.locator('a.mono').filter({ hasText: 'login' }).first().click();

  await expect(page).toHaveURL(/\/dashboard\?type=login/);
  // Active-filter badge announces the current type filter — the inner
  // mono span renders the literal `type=login` string.
  await expect(page.locator('span.ss-badge').filter({ hasText: 'type=login' })).toBeVisible();

  // Verify the server actually issued a filtered listEvents call.
  const calls = await getCalls(request);
  const filteredCall = calls
    .filter((c) => c.method === 'GET' && c.path === '/v1/me/events')
    .find((c) => c.query.includes('event_type=login'));
  expect(filteredCall, 'expected /v1/me/events?event_type=login call').toBeTruthy();
});

test('dashboard_pager_older_link_uses_smallest_seq', async ({ page, request }) => {
  // 50-event page (PAGE_LIMIT) means "Older →" renders. Smallest seq
  // in `eventsPageDescending` is 51 (100 down to 51).
  await setScenario(
    request,
    scenarioFor('dashboard_pager', {
      'GET /v1/me/summary': summaryWithEvents,
      'GET /v1/me/events': eventsPageDescending,
      'GET /v1/me/timeline': timeline30Days,
    }),
  );
  await page.goto('/dashboard');

  const olderLink = page.getByRole('link', { name: /Older/ });
  await expect(olderLink).toBeVisible();
  const href = await olderLink.getAttribute('href');
  expect(href).toContain('before_seq=51');
});
