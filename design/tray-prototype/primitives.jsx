/**
 * StarStats — shared UI primitives
 * Card, Badge, Button, Input, KV list, Toggle, Heatmap, Secret block,
 * Segmented OTP. Tokens come from tokens.css; these wrap layout +
 * behaviour only.
 */

const Card = ({ children, className = "", title, eyebrow, footer, danger, ...rest }) => (
  <section
    className={`ss-card ${danger ? "" : ""} ${className}`}
    style={danger ? { borderColor: "color-mix(in oklab, var(--danger) 35%, transparent)" } : undefined}
    {...rest}
  >
    {(title || eyebrow) && (
      <header style={{ padding: "20px 24px 0", display: "flex", alignItems: "baseline", gap: 14, justifyContent: "space-between", flexWrap: "wrap" }}>
        <div>
          {eyebrow && <div className="ss-eyebrow" style={{ marginBottom: 6 }}>{eyebrow}</div>}
          {title && <h2 style={{ margin: 0, fontSize: 17, fontWeight: 600, letterSpacing: "-0.01em", color: danger ? "var(--danger)" : "var(--fg)" }}>{title}</h2>}
        </div>
      </header>
    )}
    <div style={{ padding: "16px 24px 22px" }}>{children}</div>
    {footer && (
      <>
        <hr className="ss-rule" />
        <div style={{ padding: "14px 24px" }}>{footer}</div>
      </>
    )}
  </section>
);

const Badge = ({ kind = "neutral", children, dot = false }) => {
  const cls = {
    ok: "ss-badge ss-badge--ok",
    warn: "ss-badge ss-badge--warn",
    danger: "ss-badge ss-badge--danger",
    accent: "ss-badge ss-badge--accent",
    neutral: "ss-badge",
  }[kind] || "ss-badge";
  return (
    <span className={cls}>
      {dot && <span className="ss-badge-dot" />}
      {children}
    </span>
  );
};

const Button = ({ kind = "primary", children, icon, iconRight, ...rest }) => {
  const cls = {
    primary: "ss-btn ss-btn--primary",
    ghost: "ss-btn ss-btn--ghost",
    danger: "ss-btn ss-btn--danger",
    link: "ss-btn ss-btn--link",
  }[kind] || "ss-btn ss-btn--primary";
  return (
    <button className={cls} {...rest}>
      {icon}
      {children}
      {iconRight}
    </button>
  );
};

const Input = ({ label, hint, suffix, ...rest }) => (
  <label className="ss-label">
    {label && <span className="ss-label-text">{label}</span>}
    <span style={{ position: "relative", display: "block" }}>
      <input className="ss-input" {...rest} />
      {suffix && (
        <span style={{
          position: "absolute", right: 10, top: "50%", transform: "translateY(-50%)",
          color: "var(--fg-dim)", fontSize: 12, fontFamily: "var(--font-mono)"
        }}>{suffix}</span>
      )}
    </span>
    {hint && <small style={{ color: "var(--fg-dim)", fontSize: 12 }}>{hint}</small>}
  </label>
);

const KV = ({ items }) => (
  <dl className="ss-kv">
    {items.map(([k, v], i) => (
      <React.Fragment key={i}>
        <dt>{k}</dt>
        <dd>{v}</dd>
      </React.Fragment>
    ))}
  </dl>
);

const Toggle = ({ on, onChange, children }) => (
  <button
    type="button"
    className="ss-toggle"
    data-on={String(!!on)}
    onClick={() => onChange?.(!on)}
    style={{ background: "transparent", border: 0, padding: 0, color: "inherit" }}
  >
    <span className="ss-toggle-track">
      <span className="ss-toggle-thumb" />
    </span>
    {children && <span>{children}</span>}
  </button>
);

const Secret = ({ value, mask = true }) => {
  const [revealed, setRevealed] = React.useState(!mask);
  const [copied, setCopied] = React.useState(false);
  const display = revealed ? value : "•".repeat(Math.min(value.length, 28));
  return (
    <div className="ss-secret">
      <code className="ss-secret-code mono">{display}</code>
      <button
        type="button"
        className="ss-btn ss-btn--ghost"
        style={{ padding: "6px 10px", fontSize: 12 }}
        onClick={() => setRevealed(!revealed)}
        aria-label={revealed ? "Hide secret" : "Reveal secret"}
      >
        {revealed ? <I.eyeoff size={14} /> : <I.eye size={14} />}
        {revealed ? "Hide" : "Reveal"}
      </button>
      <button
        type="button"
        className="ss-btn ss-btn--ghost"
        style={{ padding: "6px 10px", fontSize: 12 }}
        onClick={() => { setCopied(true); setTimeout(() => setCopied(false), 1200); }}
        aria-label="Copy secret"
      >
        {copied ? <I.check size={14} /> : <I.copy size={14} />}
        {copied ? "Copied" : "Copy"}
      </button>
    </div>
  );
};

/** 6-digit segmented OTP — visual mock with focus tracking. */
const OTP = ({ values = ["", "", "", "", "", ""], focusIdx = 0 }) => (
  <div className="ss-otp">
    {values.slice(0, 3).map((v, i) => (
      <input
        key={i}
        className="ss-otp-cell mono"
        defaultValue={v}
        maxLength={1}
        inputMode="numeric"
        data-focused={focusIdx === i ? "true" : undefined}
      />
    ))}
    <span className="ss-otp-sep" />
    {values.slice(3).map((v, i) => (
      <input
        key={i + 3}
        className="ss-otp-cell mono"
        defaultValue={v}
        maxLength={1}
        inputMode="numeric"
        data-focused={focusIdx === i + 3 ? "true" : undefined}
      />
    ))}
  </div>
);

/** Calendar heatmap. Generates 26 weeks (≈6 months) of fake data
 *  with some recent activity bias. Deterministic — seeded by index. */
const Heatmap = ({ weeks = 26, seed = 7, label = "Last 6 months" }) => {
  const cells = React.useMemo(() => {
    const arr = [];
    let s = seed;
    const rng = () => { s = (s * 1664525 + 1013904223) >>> 0; return s / 0xffffffff; };
    for (let w = 0; w < weeks; w++) {
      for (let d = 0; d < 7; d++) {
        // higher density toward the end (recent)
        const recencyBoost = w / weeks; // 0 → 1
        const r = rng();
        const r2 = rng();
        let lvl = 0;
        const t = r * (0.3 + recencyBoost * 0.7);
        if (t > 0.18) lvl = 1;
        if (t > 0.32) lvl = 2;
        if (t > 0.55) lvl = 3;
        if (t > 0.78) lvl = 4;
        // a few zero gaps even recently
        if (r2 < 0.12) lvl = 0;
        arr.push(lvl);
      }
    }
    return arr;
  }, [weeks, seed]);

  // Days of week column to the left.
  return (
    <div style={{ display: "flex", gap: 12, alignItems: "flex-start" }}>
      <div style={{
        display: "grid", gridTemplateRows: "repeat(7, 14px)", gap: 3,
        fontSize: 10, color: "var(--fg-dim)", textTransform: "uppercase", letterSpacing: "0.08em"
      }}>
        {["S", "M", "T", "W", "T", "F", "S"].map((d, i) => (
          <span key={i} style={{ lineHeight: "14px", textAlign: "right", opacity: i % 2 ? 1 : 0 }}>{d}</span>
        ))}
      </div>
      <div>
        <div className="ss-heatmap" style={{ gridTemplateColumns: `repeat(${weeks}, 14px)` }}>
          {cells.map((lvl, i) => (
            <span
              key={i}
              className="ss-heatcell"
              data-l={lvl || undefined}
              title={`${label} · level ${lvl}`}
            />
          ))}
        </div>
        <div style={{
          display: "flex", justifyContent: "space-between", alignItems: "center",
          marginTop: 10, fontSize: 11, color: "var(--fg-dim)"
        }}>
          <span>{label}</span>
          <span style={{ display: "inline-flex", gap: 6, alignItems: "center" }}>
            <span>Less</span>
            {[0, 1, 2, 3, 4].map(l => (
              <span key={l} className="ss-heatcell" data-l={l || undefined} style={{ width: 10, height: 10 }} />
            ))}
            <span>More</span>
          </span>
        </div>
      </div>
    </div>
  );
};

/** Top-types ranked list with proportional bars. */
const TypeBars = ({ items, total, onSelect }) => (
  <ul style={{ listStyle: "none", margin: 0, padding: 0, display: "flex", flexDirection: "column", gap: 12 }}>
    {items.map((t) => {
      const pct = (t.count / total) * 100;
      return (
        <li key={t.type} style={{
          display: "grid",
          gridTemplateColumns: "minmax(0, 220px) 1fr 110px",
          gap: 14, alignItems: "center",
          fontVariantNumeric: "tabular-nums",
        }}>
          <button
            onClick={() => onSelect?.(t.type)}
            className="mono"
            style={{
              background: "transparent", border: 0, padding: 0,
              color: "var(--accent)", textAlign: "left",
              cursor: "pointer", fontSize: 13, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap"
            }}
          >
            {t.type}
          </button>
          <span style={{ display: "block", height: 6, background: "var(--grid-empty)", borderRadius: 3, overflow: "hidden" }}>
            <span style={{
              display: "block", height: "100%", width: `${pct}%`,
              background: "var(--accent)", borderRadius: 3,
              transition: "width 600ms var(--ease-out)"
            }} />
          </span>
          <span style={{ textAlign: "right", fontSize: 13 }}>
            {t.count.toLocaleString()}<span style={{ color: "var(--fg-dim)" }}> · {pct.toFixed(1)}%</span>
          </span>
        </li>
      );
    })}
  </ul>
);

/** Timeline list — each event a row with time, source, summary. */
const TimelineList = ({ events }) => (
  <ol style={{ listStyle: "none", margin: 0, padding: 0, display: "flex", flexDirection: "column", gap: 0 }}>
    {events.map((e, i) => (
      <li
        key={i}
        style={{
          display: "grid",
          gridTemplateColumns: "78px 90px 1fr auto",
          gap: 14, alignItems: "baseline",
          padding: "10px 12px",
          borderLeft: `2px solid ${e.color || "var(--border-strong)"}`,
          marginLeft: 4,
          fontSize: 13,
        }}
      >
        <time style={{ color: "var(--fg-dim)", fontFamily: "var(--font-mono)", fontSize: 12 }}>{e.time}</time>
        <span className="mono" style={{ color: "var(--accent)", fontSize: 11, textTransform: "uppercase", letterSpacing: "0.05em" }}>
          {e.source}
        </span>
        <span style={{ color: "var(--fg)" }}>{e.summary}</span>
        {e.tag && <Badge kind={e.tagKind || "neutral"}>{e.tag}</Badge>}
      </li>
    ))}
  </ol>
);

const Stack = ({ gap = 16, children, style, ...rest }) => (
  <div style={{ display: "flex", flexDirection: "column", gap, ...style }} {...rest}>{children}</div>
);

const Row = ({ gap = 12, align = "center", justify = "flex-start", children, style, ...rest }) => (
  <div style={{ display: "flex", gap, alignItems: align, justifyContent: justify, flexWrap: "wrap", ...style }} {...rest}>{children}</div>
);

Object.assign(window, {
  Card, Badge, Button, Input, KV, Toggle, Secret, OTP, Heatmap, TypeBars, TimelineList, Stack, Row,
});
