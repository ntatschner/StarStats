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
import { toFriendlyName } from './heuristic-name';

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
 * Resolve a raw class identifier through a category Map; on miss,
 * fall through to the heuristic prettifier so the UI never renders a
 * bare underscored identifier. Shared between per-event rendering
 * (`event-summary.ts`) and aggregate-bucket rendering on the journey
 * page — both paths need identical lookup semantics.
 */
export function prettyClass(
  raw: string | null | undefined,
  map: ReferenceMap,
): string {
  if (!raw) return '';
  return map.get(raw.toLowerCase()) ?? toFriendlyName(raw);
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

// -- Location catalog (Wave 1: catalog-driven hierarchy) --------------
//
// Locations get a richer treatment than the other categories because
// the wiki provides full system → body → place hierarchy in metadata
// (see docs/REFERENCE-CATALOG-HIERARCHY.md). We expose that to
// `parseLocationClass` so it can consult the catalog before falling
// back to the hardcoded engine-short-code dictionaries.

/** Trimmed shape of a wiki location entry — only the fields we use
 *  for hierarchy resolution. Sourced from the raw wiki JSON which
 *  the server persists verbatim into `reference_registry.metadata`. */
export interface LocationEntry {
  /** Engine join key as the server stores it. Usually the wiki
   *  `slug` (e.g. `aberdeen-2`) since the wiki has no
   *  `class_name` field for locations. */
  classKey: string;
  /** Canonical display name (`"Aberdeen"`). */
  displayName: string;
  /** Parent system display from `star.name` (`"Stanton"`). */
  system: string | null;
  /** Parent body from `parent.name`. Null when the entry IS a
   *  planet or has no parent. */
  parent: string | null;
  /** Engine-internal joined short form from `tag.name`
   *  (`"Stanton1b"`). Primary match candidate against event
   *  payloads. */
  tag: string | null;
  /** URL slug (`"aberdeen-2"`). Match fallback. */
  slug: string | null;
  /** `type.classification` — `"Star"` / `"Planet"` / `"Moon"` /
   *  `"City"` / `"Station"` / `"Outpost"`. Drives display
   *  decisions (e.g. don't render `parent` for a planet). */
  classification: string | null;
}

/** Multi-index lookup over the location catalog. Several keys per
 *  entry so the parser can match by name, by engine tag, or by slug
 *  without knowing which form an event payload uses. */
export interface LocationCatalog {
  /** Lookup by `name` (lowercased). Primary match for event tokens
   *  that already match a wiki name (`Orison`, `Lorville`). */
  byName: ReadonlyMap<string, LocationEntry>;
  /** Lookup by engine `tag.name` (lowercased). Matches joined forms
   *  like `Stanton1b` that come from system+index payloads. */
  byTag: ReadonlyMap<string, LocationEntry>;
  /** Lookup by wiki `slug` (lowercased). Matches when an event
   *  payload happens to carry a kebab-case identifier. */
  bySlug: ReadonlyMap<string, LocationEntry>;
  /** Lookup by stored `class_name` (server's primary key). Back-compat
   *  for callers that still want the legacy display-only Map. */
  display: ReferenceMap;
}

/** Empty catalog — safe default when the fetch fails or hasn't
 *  resolved yet. All lookups miss; the parser falls through to its
 *  dictionary tiers without crashing. */
export const EMPTY_LOCATION_CATALOG: LocationCatalog = {
  byName: new Map(),
  byTag: new Map(),
  bySlug: new Map(),
  display: new Map(),
};

interface RawWikiLocation {
  name?: string;
  slug?: string;
  star?: { name?: string };
  parent?: { name?: string };
  tag?: { name?: string };
  type?: { classification?: string };
}

/**
 * Fetch the location catalogue with hierarchy metadata. The
 * server-side `/v1/reference/location` endpoint already returns
 * each entry's full wiki JSON in the `metadata` field — we just
 * decode and index here.
 */
export async function getLocationCatalog(): Promise<LocationCatalog> {
  try {
    const resp = await fetch(`${apiBase()}/v1/reference/location`, {
      method: 'GET',
      next: { revalidate: 3600 },
    });
    if (!resp.ok) return EMPTY_LOCATION_CATALOG;
    const body = (await resp.json()) as ReferenceListResponse;
    const byName = new Map<string, LocationEntry>();
    const byTag = new Map<string, LocationEntry>();
    const bySlug = new Map<string, LocationEntry>();
    const display = new Map<string, string>();
    for (const raw of body.entries ?? []) {
      if (!raw.class_name) continue;
      const md = (raw.metadata ?? {}) as RawWikiLocation;
      const entry: LocationEntry = {
        classKey: raw.class_name,
        displayName: raw.display_name || raw.class_name,
        system: md.star?.name?.trim() || null,
        parent: md.parent?.name?.trim() || null,
        tag: md.tag?.name?.trim() || null,
        slug: md.slug?.trim() || null,
        classification: md.type?.classification?.trim() || null,
      };
      display.set(raw.class_name.toLowerCase(), entry.displayName);
      if (raw.display_name) {
        byName.set(raw.display_name.toLowerCase(), entry);
      }
      if (entry.tag) byTag.set(entry.tag.toLowerCase(), entry);
      if (entry.slug) bySlug.set(entry.slug.toLowerCase(), entry);
    }
    return { byName, byTag, bySlug, display };
  } catch {
    return EMPTY_LOCATION_CATALOG;
  }
}
