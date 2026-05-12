/**
 * Frontend wrapper for the server's generic reference catalogue.
 *
 * Each category (vehicle / weapon / item / location) is fetched from
 * `/v1/reference/{category}`, reduced to a `Map<lowercased class_name,
 * display_name>`, and bundled into a `ReferenceLookup` object the
 * dashboard / journey pages pass into `formatEventSummary` for
 * resolution.
 *
 * The endpoint is public + rate-limited server-side. Next.js caches
 * each category at the fetch layer for an hour so re-rendering the
 * dashboard doesn't repull the same ~150-row tables on every request.
 * Upstream refreshes once per 24h, so an hour of staleness is invisible.
 *
 * Failure mode: each category fetch independently returns an empty
 * Map on error so a partial outage degrades that category's lookups
 * to the heuristic fallback (see `heuristic-name.ts`) rather than
 * breaking the whole page.
 */

import { apiBase } from './api';

export type ReferenceCategory = 'vehicle' | 'weapon' | 'item' | 'location';

const CATEGORIES: ReadonlyArray<ReferenceCategory> = [
  'vehicle',
  'weapon',
  'item',
  'location',
];

export interface ReferenceEntry {
  category: ReferenceCategory;
  class_name: string;
  display_name: string;
  metadata?: Record<string, unknown>;
}

interface ReferenceListResponse {
  entries: ReferenceEntry[];
}

/** Map keyed by lowercased class_name → display_name. */
export type ReferenceMap = ReadonlyMap<string, string>;

/** One Map per category. Each Map is empty (not absent) on fetch
 *  failure so callers don't need a per-category presence check. */
export interface ReferenceLookup {
  vehicles: ReferenceMap;
  weapons: ReferenceMap;
  items: ReferenceMap;
  locations: ReferenceMap;
}

/** Empty lookup — safe default for callers before the fetch resolves. */
export const EMPTY_REFERENCE_LOOKUP: ReferenceLookup = {
  vehicles: new Map(),
  weapons: new Map(),
  items: new Map(),
  locations: new Map(),
};

/**
 * Fetch one category's entries and reduce to a `Map<lowercased
 * class_name, display_name>`. Returns an empty Map on any error so
 * the caller gets a stable Map shape rather than a thrown exception
 * — reference data is opt-in cosmetic, not load-bearing.
 */
export async function getReferences(
  category: ReferenceCategory,
): Promise<ReferenceMap> {
  try {
    const resp = await fetch(`${apiBase()}/v1/reference/${category}`, {
      method: 'GET',
      next: { revalidate: 3600 },
    });
    if (!resp.ok) {
      return new Map();
    }
    const body = (await resp.json()) as ReferenceListResponse;
    const map = new Map<string, string>();
    for (const e of body.entries ?? []) {
      if (e.class_name && e.display_name) {
        map.set(e.class_name.toLowerCase(), e.display_name);
      }
    }
    return map;
  } catch {
    return new Map();
  }
}

/**
 * Load all four reference categories in parallel. Each individual
 * fetch can degrade independently — a Weapon-only outage doesn't
 * affect Vehicle / Item / Location lookups.
 */
export async function loadAllReferences(): Promise<ReferenceLookup> {
  const [vehicles, weapons, items, locations] = await Promise.all(
    CATEGORIES.map((c) => getReferences(c)),
  );
  return { vehicles, weapons, items, locations };
}
