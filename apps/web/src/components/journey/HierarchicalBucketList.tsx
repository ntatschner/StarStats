/**
 * Two-level expandable bucket list for journey stats roll-ups.
 *
 * Server-rendered — uses native `<details>`/`<summary>` for the
 * collapse mechanic so no client JS ships. Layout:
 *
 *   ▸ Klaus & Werner                          237 ▮▮▮▮▮▮▮▮▮▮
 *       Laser Cannon  [S1 ×12] [S2 ×142]      154 ▮▮▮▮▮▮
 *       Laser Repeater                         75 ▮▮▮
 *     Behring                                   88 ▮▮▮
 *       P4-AR                                   88 ▮▮▮
 *
 * The component is intentionally generic over the row shape so it
 * works for weapons (mfr → family + size badges), items (mfr →
 * model), and locations (system → body → place). Callers pre-roll
 * up the buckets via the helpers below and hand the resulting tree
 * in.
 */

import {
  parseWeaponClass,
  parseItemClass,
  parseLocationClass,
} from '@/lib/class-name-parts';
import { prettyClass } from '@/lib/reference';
import type { ReferenceMap } from '@/lib/reference';

export interface RollupNode {
  /** Display label for this node (e.g. "Klaus & Werner"). */
  label: string;
  /** Cumulative count across this node and all its children. */
  count: number;
  /** Optional badges to append to the label row, e.g. size tags. */
  badges?: Array<{ text: string; count: number }>;
  /** Optional child nodes. When present, the row becomes an
   *  expandable `<details>`. */
  children?: RollupNode[];
  /** Optional tooltip — usually the raw class identifier so power
   *  users can cross-reference the wiki. */
  title?: string;
}

export function HierarchicalBucketList({
  nodes,
}: {
  nodes: RollupNode[];
}) {
  const max = Math.max(...nodes.map((n) => n.count), 1);
  return (
    <ol
      style={{
        listStyle: 'none',
        margin: 0,
        padding: 0,
        display: 'flex',
        flexDirection: 'column',
        gap: 6,
      }}
    >
      {nodes.map((n, i) => (
        <li key={`${n.label}-${i}`}>
          <TopRow node={n} max={max} />
        </li>
      ))}
    </ol>
  );
}

function TopRow({ node, max }: { node: RollupNode; max: number }) {
  const hasChildren = !!node.children && node.children.length > 0;
  const pct = (node.count / max) * 100;
  if (!hasChildren) {
    return (
      <BarRow
        label={node.label}
        count={node.count}
        pct={pct}
        badges={node.badges}
        title={node.title}
      />
    );
  }
  const childMax = Math.max(...node.children!.map((c) => c.count), 1);
  return (
    <details>
      <summary style={{ cursor: 'pointer', listStyle: 'none' }}>
        <BarRow
          label={node.label}
          count={node.count}
          pct={pct}
          badges={node.badges}
          title={node.title}
          isGroup
        />
      </summary>
      <ol
        style={{
          listStyle: 'none',
          margin: '6px 0 0 18px',
          padding: 0,
          display: 'flex',
          flexDirection: 'column',
          gap: 4,
          borderLeft: '1px solid var(--border)',
          paddingLeft: 10,
        }}
      >
        {node.children!.map((child, i) => (
          <li key={`${child.label}-${i}`}>
            <ChildRow node={child} maxChild={childMax} />
          </li>
        ))}
      </ol>
    </details>
  );
}

function ChildRow({
  node,
  maxChild,
}: {
  node: RollupNode;
  maxChild: number;
}) {
  const hasChildren = !!node.children && node.children.length > 0;
  const pct = (node.count / maxChild) * 100;
  if (!hasChildren) {
    return (
      <BarRow
        label={node.label}
        count={node.count}
        pct={pct}
        badges={node.badges}
        title={node.title}
        compact
      />
    );
  }
  const grandMax = Math.max(...node.children!.map((c) => c.count), 1);
  return (
    <details>
      <summary style={{ cursor: 'pointer', listStyle: 'none' }}>
        <BarRow
          label={node.label}
          count={node.count}
          pct={pct}
          badges={node.badges}
          title={node.title}
          isGroup
          compact
        />
      </summary>
      <ol
        style={{
          listStyle: 'none',
          margin: '4px 0 0 14px',
          padding: 0,
          display: 'flex',
          flexDirection: 'column',
          gap: 3,
          borderLeft: '1px solid var(--border)',
          paddingLeft: 10,
        }}
      >
        {node.children!.map((grand, i) => {
          const gpct = (grand.count / grandMax) * 100;
          return (
            <li key={`${grand.label}-${i}`}>
              <BarRow
                label={grand.label}
                count={grand.count}
                pct={gpct}
                badges={grand.badges}
                title={grand.title}
                compact
              />
            </li>
          );
        })}
      </ol>
    </details>
  );
}

function BarRow({
  label,
  count,
  pct,
  badges,
  title,
  isGroup = false,
  compact = false,
}: {
  label: string;
  count: number;
  pct: number;
  badges?: Array<{ text: string; count: number }>;
  title?: string;
  isGroup?: boolean;
  compact?: boolean;
}) {
  return (
    <div>
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'baseline',
          gap: 8,
          fontSize: compact ? 11 : 12,
          marginBottom: compact ? 2 : 3,
        }}
      >
        <span
          style={{
            color: 'var(--fg)',
            overflow: 'hidden',
            display: 'flex',
            gap: 6,
            alignItems: 'baseline',
            flexWrap: 'wrap',
          }}
          title={title}
        >
          {isGroup && (
            <span
              aria-hidden="true"
              style={{
                color: 'var(--fg-dim)',
                fontSize: 10,
                lineHeight: 1,
              }}
            >
              ▸
            </span>
          )}
          <span className="mono">{label}</span>
          {badges?.map((b) => (
            <span
              key={b.text}
              className="ss-badge"
              style={{
                fontSize: 10,
                padding: '1px 6px',
                fontVariant: 'tabular-nums',
              }}
            >
              {b.text} ×{b.count}
            </span>
          ))}
        </span>
        <span
          className="mono"
          style={{ color: 'var(--fg-dim)', fontVariant: 'tabular-nums' }}
        >
          {count.toLocaleString()}
        </span>
      </div>
      <div
        style={{
          height: compact ? 3 : 4,
          background: 'var(--bg-elev)',
          borderRadius: 2,
          overflow: 'hidden',
        }}
      >
        <div
          style={{
            width: `${pct}%`,
            height: '100%',
            background: 'var(--accent)',
          }}
        />
      </div>
    </div>
  );
}

// ----- Roll-up helpers --------------------------------------------

/** Group flat `{value,count}` buckets by manufacturer → family, with
 *  per-family size badges. Used by Combat > Top weapons. */
export function rollUpWeapons(
  buckets: { value: string; count: number }[],
  catalog: ReferenceMap,
): RollupNode[] {
  const tree = new Map<
    string,
    Map<
      string,
      { count: number; sizes: Map<string, number>; raws: string[] }
    >
  >();
  for (const b of buckets) {
    const w = parseWeaponClass(b.value);
    const mfr = w.manufacturer ?? 'Unknown manufacturer';
    const family = w.family;
    let mfrMap = tree.get(mfr);
    if (!mfrMap) {
      mfrMap = new Map();
      tree.set(mfr, mfrMap);
    }
    let famEntry = mfrMap.get(family);
    if (!famEntry) {
      famEntry = { count: 0, sizes: new Map(), raws: [] };
      mfrMap.set(family, famEntry);
    }
    famEntry.count += b.count;
    famEntry.raws.push(b.value);
    if (w.size) {
      famEntry.sizes.set(w.size, (famEntry.sizes.get(w.size) ?? 0) + b.count);
    }
  }
  return [...tree.entries()]
    .map(([mfr, fams]) => {
      const children: RollupNode[] = [...fams.entries()]
        .map(([family, entry]) => {
          // If the catalog has an authoritative display for the
          // first raw class in this family, prefer it — keeps
          // wiki-aligned spellings.
          const catalogLabel = prettyClass(entry.raws[0], catalog);
          const displayFamily =
            catalogLabel && catalogLabel !== entry.raws[0]
              ? stripDuplicateMfr(catalogLabel, mfr)
              : family;
          const badges = [...entry.sizes.entries()]
            .sort((a, b) => a[0].localeCompare(b[0]))
            .map(([size, count]) => ({ text: size, count }));
          return {
            label: displayFamily,
            count: entry.count,
            badges: badges.length > 0 ? badges : undefined,
            title: entry.raws.join(' · '),
          };
        })
        .sort((a, b) => b.count - a.count);
      const total = children.reduce((acc, c) => acc + c.count, 0);
      return {
        label: mfr,
        count: total,
        children,
      };
    })
    .sort((a, b) => b.count - a.count);
}

/** Group flat buckets by manufacturer → model. Used by Loadout >
 *  Most-attached items. */
export function rollUpItems(
  buckets: { value: string; count: number }[],
  catalog: ReferenceMap,
): RollupNode[] {
  const tree = new Map<
    string,
    Map<string, { count: number; raws: string[] }>
  >();
  for (const b of buckets) {
    const it = parseItemClass(b.value);
    const mfr = it.manufacturer ?? 'Unknown manufacturer';
    let mfrMap = tree.get(mfr);
    if (!mfrMap) {
      mfrMap = new Map();
      tree.set(mfr, mfrMap);
    }
    let modelEntry = mfrMap.get(it.model);
    if (!modelEntry) {
      modelEntry = { count: 0, raws: [] };
      mfrMap.set(it.model, modelEntry);
    }
    modelEntry.count += b.count;
    modelEntry.raws.push(b.value);
  }
  return [...tree.entries()]
    .map(([mfr, models]) => {
      const children: RollupNode[] = [...models.entries()]
        .map(([model, entry]) => {
          const catalogLabel = prettyClass(entry.raws[0], catalog);
          const displayModel =
            catalogLabel && catalogLabel !== entry.raws[0]
              ? stripDuplicateMfr(catalogLabel, mfr)
              : model;
          return {
            label: displayModel,
            count: entry.count,
            title: entry.raws.join(' · '),
          };
        })
        .sort((a, b) => b.count - a.count);
      const total = children.reduce((acc, c) => acc + c.count, 0);
      return { label: mfr, count: total, children };
    })
    .sort((a, b) => b.count - a.count);
}

/** Group flat buckets by system → body → place. Used by Travel and
 *  Combat (deaths_by_zone). */
export function rollUpLocations(
  buckets: { value: string; count: number }[],
  catalog: ReferenceMap,
): RollupNode[] {
  const tree = new Map<
    string,
    Map<string, Map<string, { count: number; raws: string[] }>>
  >();
  const unknownPlaces = new Map<
    string,
    { count: number; raws: string[] }
  >();
  for (const b of buckets) {
    const loc = parseLocationClass(b.value);
    if (!loc.system) {
      const key =
        loc.place ?? prettyClass(b.value, catalog) ?? b.value;
      const e = unknownPlaces.get(key) ?? { count: 0, raws: [] };
      e.count += b.count;
      e.raws.push(b.value);
      unknownPlaces.set(key, e);
      continue;
    }
    let sysMap = tree.get(loc.system);
    if (!sysMap) {
      sysMap = new Map();
      tree.set(loc.system, sysMap);
    }
    const bodyKey = loc.body ?? '(unknown body)';
    let bodyMap = sysMap.get(bodyKey);
    if (!bodyMap) {
      bodyMap = new Map();
      sysMap.set(bodyKey, bodyMap);
    }
    const placeKey = loc.place ?? bodyKey;
    let placeEntry = bodyMap.get(placeKey);
    if (!placeEntry) {
      placeEntry = { count: 0, raws: [] };
      bodyMap.set(placeKey, placeEntry);
    }
    placeEntry.count += b.count;
    placeEntry.raws.push(b.value);
  }
  const nodes: RollupNode[] = [...tree.entries()].map(([system, bodies]) => {
    const bodyChildren: RollupNode[] = [...bodies.entries()].map(
      ([body, places]) => {
        const placeChildren: RollupNode[] = [...places.entries()]
          .map(([place, entry]) => ({
            label: prettyClass(entry.raws[0], catalog) || place,
            count: entry.count,
            title: entry.raws.join(' · '),
          }))
          .sort((a, b) => b.count - a.count);
        const bodyTotal = placeChildren.reduce((a, c) => a + c.count, 0);
        // Collapse single-place bodies whose only place duplicates
        // the body name — keeps the tree compact.
        const isSinglePlaceMatchingBody =
          placeChildren.length === 1 &&
          placeChildren[0].label.toLowerCase() === body.toLowerCase();
        return {
          label: body,
          count: bodyTotal,
          children: isSinglePlaceMatchingBody ? undefined : placeChildren,
        };
      },
    );
    bodyChildren.sort((a, b) => b.count - a.count);
    const sysTotal = bodyChildren.reduce((a, c) => a + c.count, 0);
    return {
      label: system,
      count: sysTotal,
      children: bodyChildren,
    };
  });
  if (unknownPlaces.size > 0) {
    const orphanChildren: RollupNode[] = [...unknownPlaces.entries()]
      .map(([place, entry]) => ({
        label: place,
        count: entry.count,
        title: entry.raws.join(' · '),
      }))
      .sort((a, b) => b.count - a.count);
    const orphanTotal = orphanChildren.reduce((a, c) => a + c.count, 0);
    nodes.push({
      label: 'Other / unmapped',
      count: orphanTotal,
      children: orphanChildren,
    });
  }
  return nodes.sort((a, b) => b.count - a.count);
}

function stripDuplicateMfr(label: string, mfr: string): string {
  const lower = label.toLowerCase();
  const mfrLower = mfr.toLowerCase();
  if (lower.startsWith(mfrLower + ' ')) {
    return label.slice(mfr.length + 1);
  }
  return label;
}
