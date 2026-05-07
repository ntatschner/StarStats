import { expect, test } from '@playwright/test';
import {
  loginAs,
  notFound,
  publicSummaryShared,
  resetScenario,
  setScenario,
} from './helpers/api-mock';

test.beforeEach(async ({ request }) => {
  await resetScenario(request);
});

test('public_profile_renders_when_visible', async ({ page, request }) => {
  await setScenario(request, {
    __id: 'public_visible',
    routes: {
      'GET /v1/public/JohnSomeone/summary': publicSummaryShared,
    },
  });

  await page.goto('/u/JohnSomeone');

  await expect(
    page.getByRole('heading', { name: 'JohnSomeone' }),
  ).toBeVisible();
  // "Public profile" appears in an eyebrow label, a badge, and body
  // copy — target the eyebrow specifically.
  await expect(page.locator('.ss-eyebrow', { hasText: 'Public profile' })).toBeVisible();
  // Total events is rendered with locale formatting.
  await expect(page.getByText('42', { exact: false })).toBeVisible();
});

test('public_profile_404_shows_generic_message', async ({ page, request }) => {
  await setScenario(request, {
    __id: 'public_404',
    routes: {
      'GET /v1/public/Phantom/summary': notFound,
    },
  });

  // No session cookie -> the page can't fall back to the friend path,
  // so 404 surfaces the generic "not available" view.
  await page.goto('/u/Phantom');

  await expect(
    page.getByRole('heading', { name: 'Profile not available' }),
  ).toBeVisible();
  await expect(
    page.getByText(/doesn.t exist.*isn.t public.*hasn.t been shared/),
  ).toBeVisible();
});

test('public_profile_falls_back_to_friend_view_when_logged_in', async ({
  page,
  request,
}) => {
  await loginAs(page, { handle: 'TestPilot' });
  await setScenario(request, {
    __id: 'public_friend_fallback',
    routes: {
      'GET /v1/public/JohnSomeone/summary': notFound,
      'GET /v1/u/JohnSomeone/summary': publicSummaryShared,
    },
  });

  await page.goto('/u/JohnSomeone');

  await expect(
    page.getByRole('heading', { name: 'JohnSomeone' }),
  ).toBeVisible();
  // "Shared with you" appears as both an eyebrow label and a badge —
  // target the eyebrow specifically.
  await expect(page.locator('.ss-eyebrow', { hasText: 'Shared with you' })).toBeVisible();
});
