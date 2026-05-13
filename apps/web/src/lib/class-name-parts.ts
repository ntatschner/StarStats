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
 *  random first segment as a system name. */
const KNOWN_SYSTEMS = new Set([
  'Stanton',
  'Pyro',
  'Nyx',
  'Castra',
  'Terra',
  'Sol',
]);

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
 *  system / body / place. Best-effort — falls back gracefully when
 *  the format isn't recognised. */
export function parseLocationClass(raw: string): LocationParts {
  const parts = stripAndSplit(raw);
  if (parts.length === 0) {
    return { system: null, body: null, place: null, raw };
  }
  const systemIdx = parts.findIndex((p) => KNOWN_SYSTEMS.has(p));
  if (systemIdx === -1) {
    return {
      system: null,
      body: null,
      place: titleCase(splitCamelCase(parts.join(' '))),
      raw,
    };
  }
  const system = parts[systemIdx];
  const tail = parts.slice(systemIdx + 1);
  // Tail like ['2b', 'Daymar'] → body=Daymar (drop the index token).
  // Tail like ['1', 'Hurston', 'Lorville'] → body=Hurston, place=Lorville.
  // Tail like ['2', 'Crusader'] → body=Crusader.
  const namedTail = tail.filter((p) => !isPlanetIndex(p));
  let body: string | null = null;
  let place: string | null = null;
  if (namedTail.length === 1) {
    body = titleCase(splitCamelCase(namedTail[0]));
  } else if (namedTail.length >= 2) {
    body = titleCase(splitCamelCase(namedTail[0]));
    place = titleCase(splitCamelCase(namedTail.slice(1).join(' ')));
  }
  return { system, body, place, raw };
}

// ----- helpers ----------------------------------------------------

/** Drop runtime prefixes ([PROC], LandingArea_, OOC_, NPC_, AI_) and
 *  return the meaningful tokens. */
function stripAndSplit(raw: string): string[] {
  const trimmed = raw.trim().replace(/^\[[A-Z_]+\]/, '');
  return trimmed
    .split('_')
    .map((p) => p.trim())
    .filter((p) => p.length > 0 && !SKIP_SEGMENTS.has(p));
}

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
