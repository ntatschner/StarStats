/**
 * Signature StarStats background — parallax accent ribbons + soft pulse.
 * CSS-only implementation backed by `.ss-bg-warp` and `.ss-bg-pulse`
 * classes from starstats-tokens.css. Both honour `prefers-reduced-motion`
 * via the tokens stylesheet.
 *
 * The richer canvas-based 90-streak version from the design prototype
 * (`design/prototype/quantum-warp.jsx`) is deferred to Wave 13 polish.
 */
export function QuantumWarp() {
  return (
    <>
      <div className="ss-bg-warp" aria-hidden="true" />
      <div className="ss-bg-pulse" aria-hidden="true" />
    </>
  );
}
