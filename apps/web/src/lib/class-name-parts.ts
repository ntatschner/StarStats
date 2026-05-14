/**
 * Parse raw class-name identifiers into hierarchy components for
 * the journey-page roll-ups.
 *
 * The wiki reference catalog is the authoritative source for
 * display names, but for *grouping* we need machine-extractable
 * dimensions — manufacturer, model family, size for items/weapons;
 * solar-system / body / place for locations. Both can be recovered
 * deterministically from the raw class_name string the engine emits.
 *
 *   `KLWE_LaserCannon_S2`        -> mfr=Klaus & Werner, family=Laser Cannon, size=S2
 *   `behr_rifle_ballistic_lh86`  -> mfr=Behring, family=Rifle Ballistic Lh86, size=null
 *   `OOC_Stanton_2b_Daymar`      -> system=Stanton, body=Daymar
 *   `OOC_Stanton_1_Hurston`      -> system=Stanton, body=Hurston
 *   `Orison_LOC`                 -> place=Orison (no system match → top-level place)
 *
 * No throws, never returns undefined for a non-empty input — callers
 * always get a usable shape to render.
 */

// ----- shared dictionaries ----------------------------------------

/** Lift the manufacturer-prefix dictionary from heuristic-name.ts so
 *  parsing stays consistent with display rendering. Keep in sync when
 *  new manufacturers show up in event data. */
const MANUFACTURER_NAMES: Record<string, string> = {
  AEGS: 'Aegis',
  ANVL: 'Anvil',
  ARGO: 'Argo',
  BANU: 'Banu',
  CNOU: 'Consolidated Outland',
  CRUS: 'Crusader',
  DRAK: 'Drake',
  ESPR: 'Esperia',
  GAMA: 'Gatac',
  GRIN: 'Greycat',
  KRGR: 'Kruger',
  MISC: 'MISC',
  ORIG: 'Origin',
  RSI: 'RSI',
  TMBL: 'Tumbril',
  VNCL: 'Vanduul',
  XIAN: "Xi'an",
  AMRS: 'Amon & Reese',
  APAR: 'Apocalypse Arms',
  BEHR: 'Behring',
  GMNI: 'Gemini',
  HRST: 'Hurston Dynamics',
  KBAR: 'Kastak Arms',
  KLWE: 'Klaus & Werner',
  KSAR: 'Kastak Arms',
  PRAR: 'Preacher Armament',
  JOKR: 'Joker Engineering',
  MXOX: 'MaxOx',
};

/** Solar-system head tokens we recognise. The list stays short so a
 *  miss falls through to "Unknown" rather than mis-attributing a
 *  random first segment as a system name. Matched case-insensitively
 *  via `findSystem()` so `STANTON` / `stanton` / `Stanton` all
 *  resolve. The value is the canonical display form. */
const KNOWN_SYSTEMS: Record<string, string> = {
  stanton: 'Stanton',
  pyro: 'Pyro',
  nyx: 'Nyx',
  castra: 'Castra',
  terra: 'Terra',
  sol: 'Sol',
};

/** Map from a body token (case-insensitive lookup key) to its parent
 *  system. Used when the raw destination omits the system prefix —
 *  e.g. `Hurston_Lorville` should still land under Stanton. Value is
 *  the canonical body display form. */
const KNOWN_BODIES: Record<string, { system: string; display: string }> = {
  // -- Stanton planets --
  hurston: { system: 'Stanton', display: 'Hurston' },
  crusader: { system: 'Stanton', display: 'Crusader' },
  arccorp: { system: 'Stanton', display: 'ArcCorp' },
  microtech: { system: 'Stanton', display: 'microTech' },
  // -- Stanton Lagrange-point short codes (engine emits these as
  //    prefixes in destinations like `HUR_L1_Faithful_Dream`). They
  //    name the planet they orbit, so we map each to its full body. --
  hur: { system: 'Stanton', display: 'Hurston' },
  cru: { system: 'Stanton', display: 'Crusader' },
  arc: { system: 'Stanton', display: 'ArcCorp' },
  mic: { system: 'Stanton', display: 'microTech' },
  // -- Stanton moons (Hurston / Crusader / ArcCorp / microTech) --
  aberdeen: { system: 'Stanton', display: 'Aberdeen' },
  arial: { system: 'Stanton', display: 'Arial' },
  magda: { system: 'Stanton', display: 'Magda' },
  ita: { system: 'Stanton', display: 'Ita' },
  cellin: { system: 'Stanton', display: 'Cellin' },
  daymar: { system: 'Stanton', display: 'Daymar' },
  yela: { system: 'Stanton', display: 'Yela' },
  wala: { system: 'Stanton', display: 'Wala' },
  lyria: { system: 'Stanton', display: 'Lyria' },
  calliope: { system: 'Stanton', display: 'Calliope' },
  clio: { system: 'Stanton', display: 'Clio' },
  euterpe: { system: 'Stanton', display: 'Euterpe' },
  // -- Pyro planets / dwarfs --
  bloom: { system: 'Pyro', display: 'Bloom' },
  monox: { system: 'Pyro', display: 'Monox' },
  terminus: { system: 'Pyro', display: 'Terminus' },
  // -- Pyro V moons --
  adir: { system: 'Pyro', display: 'Adir' },
  vatra: { system: 'Pyro', display: 'Vatra' },
  vuur: { system: 'Pyro', display: 'Vuur' },
  fairo: { system: 'Pyro', display: 'Fairo' },
  fuego: { system: 'Pyro', display: 'Fuego' },
  ignis: { system: 'Pyro', display: 'Ignis' },
  // -- Hurston Dynamics short code (engine emits `HurDyn_*` for
  //    several Hurston-surface installations). --
  hurdyn: { system: 'Stanton', display: 'Hurston' },
  // -- System stars themselves (when the engine references the
  //    star, it does so via `<System>Star`). Treat as the system. --
  stantonstar: { system: 'Stanton', display: 'Stanton' },
  pyrostar: { system: 'Pyro', display: 'Pyro' },
  nyxstar: { system: 'Nyx', display: 'Nyx' },
  // -- Nyx bodies --
  delamar: { system: 'Nyx', display: 'Delamar' },
};

/** Map from a place token to its hierarchy. Covers the user-visible
 *  cities, stations, and outposts that show up in event payloads
 *  without a system/body prefix (e.g. `Orison_LOC`, `GrimHEX`). When
 *  the raw place spans multiple segments (`Port_Olisar`,
 *  `New_Babbage`), the joined form is also keyed so the lookup hits
 *  either way. */
const KNOWN_PLACES: Record<
  string,
  { system: string; body: string; display: string }
> = {
  // -- Stanton cities --
  lorville: { system: 'Stanton', body: 'Hurston', display: 'Lorville' },
  orison: { system: 'Stanton', body: 'Crusader', display: 'Orison' },
  area18: { system: 'Stanton', body: 'ArcCorp', display: 'Area18' },
  newbabbage: { system: 'Stanton', body: 'microTech', display: 'New Babbage' },
  babbage: { system: 'Stanton', body: 'microTech', display: 'New Babbage' },
  // -- Stanton stations --
  grimhex: { system: 'Stanton', body: 'Yela', display: 'GrimHEX' },
  portolisar: { system: 'Stanton', body: 'Crusader', display: 'Port Olisar' },
  olisar: { system: 'Stanton', body: 'Crusader', display: 'Port Olisar' },
  everusharbor: { system: 'Stanton', body: 'Hurston', display: 'Everus Harbor' },
  porttressler: { system: 'Stanton', body: 'microTech', display: 'Port Tressler' },
  baijinipoint: { system: 'Stanton', body: 'ArcCorp', display: 'Baijini Point' },
  seraphim: { system: 'Stanton', body: 'Crusader', display: 'Seraphim Station' },
  kareah: { system: 'Stanton', body: 'Crusader', display: 'Security Post Kareah' },
  // -- Pyro --
  ruinstation: { system: 'Pyro', body: 'Pyro V', display: 'Ruin Station' },
  endgame: { system: 'Pyro', body: 'Pyro V', display: 'Endgame' },
  checkmate: { system: 'Pyro', body: 'Pyro V', display: 'Checkmate' },
  rappel: { system: 'Pyro', body: 'Pyro V', display: 'Rappel' },
  starlight: { system: 'Pyro', body: 'Pyro V', display: 'Starlight Service Station' },
  // -- Nyx --
  levski: { system: 'Nyx', body: 'Delamar', display: 'Levski' },
  // -- Rayari research outposts (scattered across microTech moons).
  //    We can't tell which moon without seeing the suffix, so we
  //    attribute them to microTech generally. Drop the body level
  //    when a specific moon is known. --
  rayari: { system: 'Stanton', body: 'microTech', display: 'Rayari Outpost' },
};

/** Match S2 / Mk3 / V1 etc. — uppercased on render. */
const SIZE_PATTERNS: RegExp[] = [
  /^S\d+$/i,
  /^M\d+$/i,
  /^Mk\d+$/i,
  /^V\d+$/i,
  /^F\d+$/i,
];

/** Tokens that don't carry semantic weight in event data — strip
 *  before grouping. */
const SKIP_SEGMENTS = new Set([
  'NPC',
  'AI',
  'OOC',
  'PROC',
  'LandingArea',
  'LOC',
]);

// ----- weapon / item parsing --------------------------------------

export interface WeaponParts {
  manufacturer: string | null;
  family: string;
  /** S2 / Mk3 / etc. — null when the class has no recognised size tag. */
  size: string | null;
  /** Raw class for fall-through display + tooltips. */
  raw: string;
}

/** Parse a weapon-class identifier into manufacturer / family / size.
 *  Used to group Combat > Top weapons by mfr and to attach size badges
 *  on the family row. */
export function parseWeaponClass(raw: string): WeaponParts {
  const parts = stripAndSplit(raw);
  const { manufacturer, rest } = lookupManufacturer(parts);
  const { sizes, nonSize } = pickSizes(rest);
  const family =
    nonSize.length > 0
      ? titleCase(splitCamelCase(nonSize.join(' ')))
      : 'Unknown';
  return {
    manufacturer,
    family,
    size: sizes[0] ?? null,
    raw,
  };
}

export interface ItemParts {
  manufacturer: string | null;
  /** Single-row display label for the item — family + variant joined,
   *  size tag preserved. */
  model: string;
  raw: string;
}

/** Item classes have inconsistent shapes (`klwe_pistol_energy_*`,
 *  `rsi_odyssey_undersuit_*`, etc.), so we don't try to split
 *  family/variant — just lift the manufacturer and present everything
 *  else as one model string. The grouping dimension is manufacturer. */
export function parseItemClass(raw: string): ItemParts {
  const parts = stripAndSplit(raw);
  const { manufacturer, rest } = lookupManufacturer(parts);
  const model =
    rest.length > 0
      ? rest
          .map((p) =>
            isSize(p) ? p.toUpperCase() : titleCase(splitCamelCase(p)),
          )
          .join(' ')
      : 'Unknown';
  return { manufacturer, model, raw };
}

// ----- location parsing -------------------------------------------

export interface LocationParts {
  system: string | null;
  body: string | null;
  place: string | null;
  raw: string;
}

/** Parse a location / destination / planet identifier into
 *  system / body / place. Best-effort — three tiers, in order:
 *    1. Leading-system match (`OOC_Stanton_2_Crusader`)
 *    2. Body match (`Hurston_Lorville` → infer Stanton)
 *    3. Place match (`Orison_LOC` → infer Stanton/Crusader/Orison)
 *  Only falls through to "no system" for truly unrecognised tokens. */
export function parseLocationClass(raw: string): LocationParts {
  const parts = stripAndSplit(raw);
  if (parts.length === 0) {
    return { system: null, body: null, place: null, raw };
  }

  // Tier 1 — system token in the segments.
  const systemIdx = parts.findIndex((p) => KNOWN_SYSTEMS[p.toLowerCase()]);
  if (systemIdx !== -1) {
    const system = KNOWN_SYSTEMS[parts[systemIdx].toLowerCase()];
    const tail = parts.slice(systemIdx + 1).filter((p) => !isPlanetIndex(p));
    return resolveAfterSystem(system, tail, raw);
  }

  // Tier 2 — body token anywhere in the segments.
  const namedParts = parts.filter((p) => !isPlanetIndex(p));
  const bodyIdx = namedParts.findIndex((p) => KNOWN_BODIES[p.toLowerCase()]);
  if (bodyIdx !== -1) {
    const bodyMeta = KNOWN_BODIES[namedParts[bodyIdx].toLowerCase()];
    const after = namedParts.slice(bodyIdx + 1);
    const place =
      after.length > 0 ? resolvePlace(after) : null;
    return {
      system: bodyMeta.system,
      body: bodyMeta.display,
      place,
      raw,
    };
  }

  // Tier 3 — place token anywhere in the segments. Try the longest
  // joined form first so multi-word place names match before their
  // individual tokens (`Port_Olisar` should win over `Olisar`).
  const placeHit = findPlaceMatch(namedParts);
  if (placeHit) {
    return {
      system: placeHit.system,
      body: placeHit.body,
      place: placeHit.display,
      raw,
    };
  }

  // No tier matched — keep the title-cased whole as a place under
  // "Other / unmapped". Genuinely unrecognised destinations.
  return {
    system: null,
    body: null,
    place: titleCase(splitCamelCase(parts.join(' '))),
    raw,
  };
}

/** Tail after the system was matched — figure out body / place. */
function resolveAfterSystem(
  system: string,
  namedTail: string[],
  raw: string,
): LocationParts {
  if (namedTail.length === 0) {
    return { system, body: null, place: null, raw };
  }
  // Prefer a known-body match in the tail. Otherwise the first
  // segment is treated as the body (matches legacy behavior).
  const bodyIdx = namedTail.findIndex((p) => KNOWN_BODIES[p.toLowerCase()]);
  if (bodyIdx !== -1) {
    const bodyMeta = KNOWN_BODIES[namedTail[bodyIdx].toLowerCase()];
    const after = namedTail.slice(bodyIdx + 1);
    return {
      system,
      body: bodyMeta.display,
      place: after.length > 0 ? resolvePlace(after) : null,
      raw,
    };
  }
  if (namedTail.length === 1) {
    return {
      system,
      body: titleCase(splitCamelCase(namedTail[0])),
      place: null,
      raw,
    };
  }
  return {
    system,
    body: titleCase(splitCamelCase(namedTail[0])),
    place: titleCase(splitCamelCase(namedTail.slice(1).join(' '))),
    raw,
  };
}

/** Render a place from one or more trailing segments. Prefers a
 *  catalog hit (so `Olisar` becomes "Port Olisar") and falls back to
 *  title-cased camel-case splitting. */
function resolvePlace(parts: string[]): string {
  // Try the joined form first (`Port_Olisar` → `portolisar` key).
  const joined = parts.join('').toLowerCase();
  if (KNOWN_PLACES[joined]) {
    return KNOWN_PLACES[joined].display;
  }
  // Then any single segment (`Olisar` → `olisar` key).
  for (const p of parts) {
    const hit = KNOWN_PLACES[p.toLowerCase()];
    if (hit) return hit.display;
  }
  return titleCase(splitCamelCase(parts.join(' ')));
}

/** Walk segments looking for a place match (single or joined).
 *  Returns the full hierarchy when found. */
function findPlaceMatch(
  parts: string[],
):
  | { system: string; body: string; display: string }
  | null {
  // Try the full joined form first (catches `Port_Olisar`,
  // `New_Babbage` even when there's no system/body context).
  const joined = parts.join('').toLowerCase();
  if (KNOWN_PLACES[joined]) return KNOWN_PLACES[joined];
  // Then each individual segment.
  for (const p of parts) {
    const hit = KNOWN_PLACES[p.toLowerCase()];
    if (hit) return hit;
  }
  return null;
}

// ----- helpers ----------------------------------------------------

/** Drop runtime prefixes ([PROC], LandingArea_, OOC_, NPC_, AI_) and
 *  return the meaningful tokens. Exported so rollup helpers can
 *  re-tokenize unmapped raws to derive a grouping key.
 *
 *  Splits joined `<System><index>` tokens like `Stanton2_X` or
 *  `Pyro4a_Y` into `[System, index, ...]` so downstream tier
 *  lookups see the system as its own segment. The engine emits
 *  these joined forms inconsistently across destination payloads. */
export function stripAndSplit(raw: string): string[] {
  const trimmed = raw.trim().replace(/^\[[A-Z_]+\]/, '');
  const segments = trimmed
    .split('_')
    .map((p) => p.trim())
    .filter((p) => p.length > 0 && !SKIP_SEGMENTS.has(p));
  // Rewrite any `<System><digit>...` segment into separate tokens.
  // Keeps the order so later positional logic (system found at idx
  // N → tail starts at N+1) still works.
  const expanded: string[] = [];
  for (const s of segments) {
    const split = splitSystemIndex(s);
    expanded.push(...split);
  }
  return expanded;
}

/** Split a token like `Stanton2`, `Stanton4a`, or `Pyro5` into
 *  `[System, index]`. Returns the input unchanged when no match. */
function splitSystemIndex(segment: string): string[] {
  // Build the alternation once. KNOWN_SYSTEMS keys are lowercase so
  // we match case-insensitively; the canonical display is recovered
  // by the downstream system lookup.
  const match = segment.match(SYSTEM_INDEX_REGEX);
  if (!match) return [segment];
  return [match[1], match[2]];
}

const SYSTEM_INDEX_REGEX = new RegExp(
  `^(${Object.keys(KNOWN_SYSTEMS).join('|')})(\\d+[a-z]?)$`,
  'i',
);

/** Filter for destination buckets — returns true when the raw value
 *  is an engine-internal marker that shouldn't appear in a
 *  destinations list at all (mission objectives, nav points, object
 *  containers, party member markers). Callers drop these before
 *  rolling up so they never leak into the bucket bars. */
export function isNonDestination(raw: string): boolean {
  const v = raw.trim();
  return NON_DESTINATION_PATTERNS.some((re) => re.test(v));
}

const NON_DESTINATION_PATTERNS: RegExp[] = [
  // Mission objective markers — `Mission_Marker_*`, `Mission_*`.
  /^Mission(_|$)/i,
  // Engine container references — `ObjectContainer*`.
  /^ObjectContainer/i,
  // HUD nav-point markers placed manually by the pilot.
  /^NavPoint/i,
  // Party member / friend markers.
  /^PartyMember/i,
];

function lookupManufacturer(parts: string[]): {
  manufacturer: string | null;
  rest: string[];
} {
  if (parts.length === 0) return { manufacturer: null, rest: parts };
  const headUpper = parts[0].toUpperCase();
  const mfr = MANUFACTURER_NAMES[headUpper] ?? null;
  return {
    manufacturer: mfr,
    rest: mfr ? parts.slice(1) : parts,
  };
}

function isSize(s: string): boolean {
  return SIZE_PATTERNS.some((re) => re.test(s));
}

/** Separate size tags from the rest — sizes get pulled to a separate
 *  list so the family name stays clean ("Laser Cannon" not "Laser
 *  Cannon S2"). */
function pickSizes(parts: string[]): {
  sizes: string[];
  nonSize: string[];
} {
  const sizes: string[] = [];
  const nonSize: string[] = [];
  for (const p of parts) {
    if (isSize(p)) sizes.push(p.toUpperCase());
    else nonSize.push(p);
  }
  return { sizes, nonSize };
}

/** Planet-index tokens are lowercase digit-letter combos like '1',
 *  '2b', '4a'. They sit between the system and the body and don't
 *  carry useful display info. */
function isPlanetIndex(s: string): boolean {
  return /^\d+[a-z]?$/i.test(s);
}

function splitCamelCase(s: string): string {
  return s.replace(/([a-z])([A-Z])/g, '$1 $2');
}

function titleCase(s: string): string {
  return s
    .split(/\s+/)
    .map((w) =>
      w.length === 0 ? w : w[0].toUpperCase() + w.slice(1).toLowerCase(),
    )
    .join(' ');
}
