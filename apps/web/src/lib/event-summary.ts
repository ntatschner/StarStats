/**
 * Per-variant pretty rendering of a `GameEvent` JSON payload.
 *
 * Mirrors `format_summary` in `crates/starstats-client/src/commands.rs`
 * — keep the two in sync. The duplication is intentional: server-side
 * rendering on the web stays self-contained without an extra fetch
 * back to a Rust formatter, and the cost is one matched function.
 *
 * The payload shape comes from `#[serde(tag = "type", rename_all =
 * "snake_case")]` on `GameEvent` in `starstats-core`, so each variant
 * is a flat object with a `type` discriminator.
 */

interface BaseEvent {
  type: string;
  timestamp: string;
}

type GameEventPayload =
  | (BaseEvent & { type: 'process_init' })
  | (BaseEvent & { type: 'legacy_login'; handle: string })
  | (BaseEvent & {
      type: 'join_pu';
      address: string;
      port: number;
      shard: string;
      location_id: string;
    })
  | (BaseEvent & { type: 'change_server'; phase: 'start' | 'end' })
  | (BaseEvent & {
      type: 'seed_solar_system';
      solar_system: string;
      shard: string;
    })
  | (BaseEvent & {
      type: 'resolve_spawn';
      player_geid: string;
      fallback: boolean;
    })
  | (BaseEvent & {
      type: 'actor_death';
      victim: string;
      killer: string;
      weapon: string;
      damage_type: string;
    })
  | (BaseEvent & {
      type: 'vehicle_destruction';
      vehicle_class: string;
      destroy_level: number;
      caused_by: string;
    })
  | (BaseEvent & { type: 'hud_notification'; text: string })
  | (BaseEvent & {
      type: 'location_inventory_requested';
      player: string;
      location: string;
    })
  | (BaseEvent & { type: 'planet_terrain_load'; planet: string })
  | (BaseEvent & {
      type: 'quantum_target_selected';
      phase: 'fuel_requested' | 'selected';
      vehicle_class: string;
      destination: string;
    })
  | (BaseEvent & {
      type: 'attachment_received';
      item_class: string;
      port: string;
    })
  | (BaseEvent & {
      type: 'vehicle_stowed';
      vehicle_id: string;
      landing_area: string;
    })
  | (BaseEvent & {
      type: 'burst_summary';
      rule_id: string;
      size: number;
      end_timestamp: string;
      anchor_body_sample?: string | null;
    });

/**
 * Format a payload into a one-liner summary. Falls back to a generic
 * "{type} event" if the payload doesn't match any known variant — that
 * way new server-side variants don't crash the dashboard, they just
 * render bare until the formatter learns them.
 *
 * The optional `vehicleNames` lookup map maps raw class names (e.g.
 * `CRUS_Starfighter_Ion`) to display names; when provided, vehicle
 * variants render the friendly name. When omitted, the raw class name
 * is preserved.
 */
export function formatEventSummary(
  payload: unknown,
  vehicleNames?: ReadonlyMap<string, string>,
): string {
  if (!isGameEventPayload(payload)) {
    if (
      typeof payload === 'object' &&
      payload !== null &&
      'type' in payload &&
      typeof (payload as { type: unknown }).type === 'string'
    ) {
      return `${(payload as { type: string }).type} event`;
    }
    return 'unknown event';
  }
  return formatKnown(payload, vehicleNames);
}

// The lookup map is keyed by lowercased class_name so log-source case
// drift (`AEGS_Avenger` vs `aegs_avenger`) resolves the same way.
function prettyVehicle(
  cls: string,
  vehicleNames: ReadonlyMap<string, string> | undefined,
): string {
  return vehicleNames?.get(cls.toLowerCase()) ?? cls;
}

function formatKnown(
  event: GameEventPayload,
  vehicleNames: ReadonlyMap<string, string> | undefined,
): string {
  switch (event.type) {
    case 'process_init':
      return 'Game process started';
    case 'legacy_login':
      return `Logged in as ${event.handle}`;
    case 'join_pu':
      return `Joined PU shard ${event.shard} (${event.address}:${event.port})`;
    case 'change_server':
      return `Server transition: ${event.phase === 'start' ? 'starting' : 'complete'}`;
    case 'seed_solar_system':
      return `Seeded ${event.solar_system} on shard ${event.shard}`;
    case 'resolve_spawn':
      return `Spawn resolved (player ${event.player_geid}, fallback=${event.fallback})`;
    case 'actor_death':
      return `${event.victim} killed by ${event.killer} (${event.weapon}, ${event.damage_type})`;
    case 'vehicle_destruction':
      return `Vehicle destroyed: ${prettyVehicle(event.vehicle_class, vehicleNames)} (level ${event.destroy_level}, by ${event.caused_by})`;
    case 'hud_notification':
      return `HUD: ${event.text.replace(/:\s*$/, '').replace(/:$/, '')}`;
    case 'location_inventory_requested':
      if (event.location === 'INVALID_LOCATION_ID') {
        return `${event.player} opened inventory (no location bound yet)`;
      }
      return `${event.player} opened inventory at ${event.location}`;
    case 'planet_terrain_load': {
      const label = event.planet.split('_').pop() ?? event.planet;
      return `Near planet/moon: ${label}`;
    }
    case 'quantum_target_selected': {
      const phase =
        event.phase === 'fuel_requested' ? 'fuel calc' : 'selected';
      return `Quantum target ${phase}: ${prettyVehicle(event.vehicle_class, vehicleNames)} → ${event.destination}`;
    }
    case 'attachment_received':
      return `Attached ${event.item_class} to ${event.port}`;
    case 'vehicle_stowed': {
      const area = event.landing_area
        .replace(/^\[PROC\]/, '')
        .replace(/^LandingArea_/, '');
      return `Ship ${event.vehicle_id} stowed at ${area}`;
    }
    case 'burst_summary': {
      // Friendly per-rule labels for the four built-in BurstRules in
      // `crates/starstats-client/src/burst_rules.rs`. Falls back to a
      // generic "Burst" for any future remote-served rule we don't
      // know about, so the timeline never renders blank.
      const label =
        event.rule_id === 'loadout_restore_burst'
          ? 'Loadout restored'
          : event.rule_id === 'terrain_load_burst'
            ? 'Terrain loaded'
            : event.rule_id === 'hud_notification_burst'
              ? 'Notifications'
              : event.rule_id === 'vehicle_stowed_burst'
                ? 'Vehicles stowed'
                : 'Burst';
      return `${label} (${event.size} events)`;
    }
  }
}

function isGameEventPayload(p: unknown): p is GameEventPayload {
  return (
    typeof p === 'object' &&
    p !== null &&
    'type' in p &&
    typeof (p as { type: unknown }).type === 'string'
  );
}
