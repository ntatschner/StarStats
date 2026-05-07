/**
 * StarStats — Donate / Support screen.
 * Two donation options: one-off £5, monthly £1.
 * Accolades + custom name plate + extended retention.
 */

const DONATE_HANDLE_DEMO = "TheCodeSaiyan";

const Donate = ({ go }) => {
  const [tier, setTier] = React.useState("monthly"); // monthly | oneoff
  const [plate, setPlate] = React.useState("Code Saiyan");
  const [accent, setAccent] = React.useState("amber"); // amber | coral | teal | violet
  const [showThanks, setShowThanks] = React.useState(false);

  const isMonthly = tier === "monthly";

  const accentColors = {
    amber:  "#E8A23C",
    coral:  "#FF6B5B",
    teal:   "#3DB5A3",
    violet: "#A38CFF",
  };

  return (
    <Stack gap={24}>
      <header>
        <div className="ss-eyebrow" style={{ marginBottom: 8, color: "var(--accent)" }}>
          <I.heart size={11} style={{ verticalAlign: "-1px", marginRight: 6 }} />
          Support StarStats
        </div>
        <h1 style={{ margin: 0, fontSize: 32, fontWeight: 600, letterSpacing: "-0.02em" }}>
          Keep the lights on, get a few perks back.
        </h1>
        <p style={{ margin: "8px 0 0", color: "var(--fg-muted)", fontSize: 14, lineHeight: 1.55, maxWidth: 720 }}>
          StarStats is run by one person and a small server. It will always be free to use — donations cover hosting, the occasional weekend rebuild, and a coffee or two. In return, supporters get a community accolade, a custom name plate on their public profile, and (for monthly supporters) extended history.
        </p>
      </header>

      {/* Tier selector */}
      <div data-rspgrid="2" style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16 }}>
        <DonateCard
          active={tier === "oneoff"}
          onClick={() => setTier("oneoff")}
          eyebrow="One-off"
          price="£5"
          cadence="one time"
          tag="Tip the jar"
          perks={[
            { icon: I.check, text: "Supporter accolade on your public profile", on: true },
            { icon: I.check, text: "Custom name plate (one line, your wording)", on: true },
            { icon: I.check, text: "Early access to new dashboard features", on: true },
            { icon: I.x,     text: "5-year data retention", on: false, hint: "Monthly only" },
            { icon: I.x,     text: "Profile theme accents", on: false, hint: "Monthly only" },
          ]}
        />
        <DonateCard
          active={tier === "monthly"}
          onClick={() => setTier("monthly")}
          eyebrow="Monthly"
          price="£1"
          cadence="per month"
          tag="Best value"
          highlight
          perks={[
            { icon: I.check, text: "Everything in the one-off tier", on: true },
            { icon: I.check, text: "Data retention extended to 5 years (from 12 months)", on: true, accent: true },
            { icon: I.check, text: "Profile theme accents (Pyro, Terra, Nyx variants)", on: true },
            { icon: I.check, text: "Priority on feature requests", on: true },
            { icon: I.check, text: "Cancel anytime — keep the accolade and name plate", on: true },
          ]}
        />
      </div>

      {/* Preview + form */}
      <div data-rspgrid="2" style={{ display: "grid", gridTemplateColumns: "1.05fr 1fr", gap: 16 }}>
        <Card eyebrow="Preview" title="How your profile changes"
          footer={<span style={{ color: "var(--fg-dim)", fontSize: 12 }}>
            Both tiers unlock the accolade + name plate. Monthly also unlocks the theme accent and extended history.
          </span>}>
          <DonatePreview
            plate={plate}
            accentColor={accentColors[accent]}
            accentName={accent}
            isMonthly={isMonthly}
          />
        </Card>

        <Card eyebrow="Customise" title={isMonthly ? "Monthly · £1 / month" : "One-off · £5"}>
          <Stack gap={16}>
            <Input
              label="Name plate"
              value={plate}
              onChange={(e) => setPlate(e.target.value.slice(0, 28))}
              hint={`${plate.length}/28 — appears under your handle on your public profile.`}
            />

            <div style={{ opacity: isMonthly ? 1 : 0.45, pointerEvents: isMonthly ? "auto" : "none", transition: "opacity 200ms" }}>
              <span className="ss-label-text" style={{ display: "block", marginBottom: 8 }}>
                Profile theme accent
                {!isMonthly && <span style={{ color: "var(--fg-dim)", fontWeight: 400, marginLeft: 8 }}>· monthly only</span>}
              </span>
              <Row gap={8}>
                {[
                  { id: "amber",  label: "Amber" },
                  { id: "coral",  label: "Coral" },
                  { id: "teal",   label: "Teal"  },
                  { id: "violet", label: "Violet"},
                ].map(opt => (
                  <button
                    key={opt.id}
                    type="button"
                    onClick={() => setAccent(opt.id)}
                    style={{
                      flex: 1,
                      padding: "10px 8px",
                      background: "var(--bg-elev)",
                      border: `1px solid ${accent === opt.id ? accentColors[opt.id] : "var(--border)"}`,
                      borderRadius: "var(--r-sm)",
                      color: "var(--fg)", fontSize: 12, cursor: "pointer",
                      display: "flex", flexDirection: "column", alignItems: "center", gap: 6,
                      boxShadow: accent === opt.id ? `0 0 0 3px color-mix(in oklab, ${accentColors[opt.id]} 22%, transparent)` : "none",
                      transition: "all 160ms",
                    }}
                  >
                    <span style={{
                      width: 22, height: 22, borderRadius: 999,
                      background: accentColors[opt.id],
                      boxShadow: `0 0 14px ${accentColors[opt.id]}66`,
                    }} />
                    {opt.label}
                  </button>
                ))}
              </Row>
            </div>

            <hr className="ss-rule" />

            <Stack gap={6}>
              <Row justify="space-between">
                <span style={{ color: "var(--fg-muted)", fontSize: 13 }}>
                  {isMonthly ? "Charged today" : "Charged once"}
                </span>
                <span className="mono" style={{ fontSize: 18, color: "var(--fg)" }}>
                  {isMonthly ? "£1.00" : "£5.00"}
                </span>
              </Row>
              {isMonthly && (
                <Row justify="space-between">
                  <span style={{ color: "var(--fg-dim)", fontSize: 12 }}>Then £1.00 every month — cancel anytime.</span>
                </Row>
              )}
            </Stack>

            <Button
              kind="primary"
              icon={<I.heart size={13} />}
              onClick={() => setShowThanks(true)}
              style={{ alignSelf: "stretch", justifyContent: "center" }}
            >
              {isMonthly ? "Start monthly support · £1" : "Send a one-off · £5"}
            </Button>

            <p style={{ margin: 0, color: "var(--fg-dim)", fontSize: 11, lineHeight: 1.5 }}>
              Payment goes through Stripe. We don't store card details. Cancel monthly any time from this page — your accolade and name plate stay; data retention drops back to 12 months at the next billing cycle.
            </p>
          </Stack>
        </Card>
      </div>

      {/* What changes when you stop */}
      <Card eyebrow="If you cancel monthly" title="What you keep, what reverts">
        <div data-rspgrid="2" style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 24 }}>
          <Stack gap={10}>
            <span style={{ fontWeight: 600, color: "var(--ok, var(--fg))", display: "flex", alignItems: "center", gap: 8 }}>
              <Badge kind="ok" dot>Keeps</Badge>
            </span>
            <KeepRevertItem ok text="Supporter accolade on your profile" />
            <KeepRevertItem ok text="Custom name plate (your existing wording stays)" />
            <KeepRevertItem ok text="Early access flag" />
            <KeepRevertItem ok text="All your historical events from your active support window" />
          </Stack>
          <Stack gap={10}>
            <span style={{ fontWeight: 600, color: "var(--fg)", display: "flex", alignItems: "center", gap: 8 }}>
              <Badge kind="warn" dot>Reverts</Badge>
            </span>
            <KeepRevertItem text="Retention window drops 5y → 12 months at next billing cycle" />
            <KeepRevertItem text="Profile theme accent goes back to default amber" />
            <KeepRevertItem text="Events older than 12 months are pseudonymised, then purged after 30-day grace" />
          </Stack>
        </div>
      </Card>

      {/* Where the money goes */}
      <Card eyebrow="Where it goes" title="A quick, honest breakdown">
        <div data-rspgrid="4" style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: 12 }}>
          {[
            { pct: 55, label: "Hosting + database", note: "Hetzner box + S3 cold storage" },
            { pct: 25, label: "Domain + email",      note: "Comm-Link delivery, magic links" },
            { pct: 15, label: "Coffee for the dev",  note: "Honestly" },
            { pct:  5, label: "Stripe fees",         note: "The unavoidable bit" },
          ].map(b => (
            <div key={b.label} className="ss-card" style={{ padding: 16, background: "var(--bg-elev)" }}>
              <div className="mono" style={{ fontSize: 22, color: "var(--accent)", letterSpacing: "-0.01em" }}>
                {b.pct}%
              </div>
              <div style={{ fontSize: 13, fontWeight: 600, marginTop: 4, color: "var(--fg)" }}>
                {b.label}
              </div>
              <div style={{ fontSize: 11, color: "var(--fg-dim)", marginTop: 4, lineHeight: 1.4 }}>
                {b.note}
              </div>
            </div>
          ))}
        </div>
      </Card>

      {/* Confirmation modal-ish */}
      {showThanks && (
        <div
          onClick={() => setShowThanks(false)}
          style={{
            position: "fixed", inset: 0, background: "rgba(0,0,0,0.55)",
            display: "grid", placeItems: "center", zIndex: 100,
            backdropFilter: "blur(4px)",
          }}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="ss-card ss-card--elev"
            style={{
              maxWidth: 460, padding: 32, textAlign: "center",
              border: "1px solid var(--accent)",
              boxShadow: "0 30px 80px rgba(0,0,0,0.6), 0 0 60px var(--accent-glow)",
            }}
          >
            <div style={{
              width: 56, height: 56, margin: "0 auto 16px",
              borderRadius: 999, background: "var(--accent-soft)",
              display: "grid", placeItems: "center", color: "var(--accent)",
              border: "1px solid color-mix(in oklab, var(--accent) 40%, transparent)",
            }}>
              <I.heart size={24} />
            </div>
            <h2 style={{ margin: 0, fontSize: 22, fontWeight: 600, letterSpacing: "-0.01em" }}>
              Thank you, {DONATE_HANDLE_DEMO}.
            </h2>
            <p style={{ margin: "10px 0 20px", color: "var(--fg-muted)", fontSize: 14, lineHeight: 1.55 }}>
              {isMonthly
                ? "Your monthly support is active. Accolade, name plate, theme accent, and 5-year retention are now live on your profile."
                : "Your one-off donation is in. Accolade and name plate are now live on your profile. Thanks for the coffee."}
            </p>
            <Row gap={8} justify="center">
              <Button kind="ghost" onClick={() => setShowThanks(false)}>Close</Button>
              <Button kind="primary" onClick={() => { setShowThanks(false); go("profile"); }} iconRight={<I.arrow size={13} />}>
                See my profile
              </Button>
            </Row>
          </div>
        </div>
      )}
    </Stack>
  );
};

/* ============================================================
 * Donate-tier card
 * ========================================================== */
const DonateCard = ({ active, onClick, eyebrow, price, cadence, tag, perks, highlight }) => (
  <button
    type="button"
    onClick={onClick}
    className="ss-card"
    style={{
      textAlign: "left", cursor: "pointer", padding: 24,
      background: active ? "var(--surface)" : "var(--bg-elev)",
      border: `1px solid ${active ? "var(--accent)" : "var(--border)"}`,
      boxShadow: active ? "0 0 0 3px color-mix(in oklab, var(--accent) 18%, transparent)" : "none",
      transition: "all 180ms",
      position: "relative",
      color: "inherit", font: "inherit",
    }}
  >
    {tag && (
      <span style={{
        position: "absolute", top: 16, right: 16,
        fontSize: 10, letterSpacing: "0.08em", textTransform: "uppercase",
        padding: "4px 8px", borderRadius: 999,
        background: highlight ? "var(--accent)" : "var(--bg)",
        color: highlight ? "var(--accent-on)" : "var(--fg-muted)",
        border: highlight ? "none" : "1px solid var(--border)",
        fontWeight: 600,
      }}>{tag}</span>
    )}
    <div className="ss-eyebrow" style={{ color: active ? "var(--accent)" : "var(--fg-dim)" }}>{eyebrow}</div>
    <div style={{ display: "flex", alignItems: "baseline", gap: 8, marginTop: 10 }}>
      <span style={{ fontSize: 40, fontWeight: 600, letterSpacing: "-0.02em", color: "var(--fg)", fontFamily: "var(--font-mono)" }}>{price}</span>
      <span style={{ fontSize: 13, color: "var(--fg-muted)" }}>{cadence}</span>
    </div>
    <hr className="ss-rule" style={{ margin: "16px 0" }} />
    <Stack gap={9}>
      {perks.map((p, i) => (
        <Row key={i} gap={10} style={{ flexWrap: "nowrap", alignItems: "flex-start" }}>
          <span style={{
            width: 18, height: 18, borderRadius: 999,
            background: p.on ? (p.accent ? "var(--accent)" : "color-mix(in oklab, var(--accent) 22%, transparent)") : "var(--bg)",
            color: p.on ? (p.accent ? "var(--accent-on)" : "var(--accent)") : "var(--fg-dim)",
            border: `1px solid ${p.on ? (p.accent ? "var(--accent)" : "color-mix(in oklab, var(--accent) 30%, transparent)") : "var(--border)"}`,
            display: "grid", placeItems: "center", flexShrink: 0,
            marginTop: 1,
          }}>
            <p.icon size={10} />
          </span>
          <Stack gap={2}>
            <span style={{
              fontSize: 13,
              color: p.on ? "var(--fg)" : "var(--fg-dim)",
              textDecoration: p.on ? "none" : "line-through",
              textDecorationColor: "color-mix(in oklab, var(--fg-dim) 60%, transparent)",
              fontWeight: p.accent ? 600 : 400,
            }}>{p.text}</span>
            {p.hint && (
              <span style={{ fontSize: 11, color: "var(--fg-dim)" }}>{p.hint}</span>
            )}
          </Stack>
        </Row>
      ))}
    </Stack>
  </button>
);

/* ============================================================
 * Profile preview block
 * ========================================================== */
const DonatePreview = ({ plate, accentColor, accentName, isMonthly }) => (
  <div
    style={{
      position: "relative",
      padding: 22,
      borderRadius: "var(--r-md)",
      background: `linear-gradient(135deg, color-mix(in oklab, ${accentColor} 14%, var(--bg-elev)) 0%, var(--bg-elev) 60%)`,
      border: `1px solid color-mix(in oklab, ${accentColor} 40%, var(--border))`,
      overflow: "hidden",
    }}
  >
    <div style={{
      position: "absolute", inset: -40,
      background: `radial-gradient(circle at 80% 20%, ${accentColor}33, transparent 55%)`,
      filter: "blur(20px)", pointerEvents: "none",
    }} />
    <Row gap={16} style={{ position: "relative", flexWrap: "nowrap", alignItems: "flex-start" }}>
      {/* Avatar */}
      <div style={{
        width: 64, height: 64, borderRadius: "var(--r-md)",
        background: `linear-gradient(135deg, ${accentColor}, color-mix(in oklab, ${accentColor} 50%, #000))`,
        display: "grid", placeItems: "center",
        color: "#fff", fontSize: 24, fontWeight: 600,
        boxShadow: `0 8px 24px ${accentColor}55`,
        flexShrink: 0,
      }}>
        TS
      </div>
      <Stack gap={4} style={{ flex: 1, minWidth: 0 }}>
        <Row gap={8} style={{ flexWrap: "nowrap", alignItems: "center" }}>
          <span style={{ fontSize: 18, fontWeight: 600, color: "var(--fg)" }}>{DONATE_HANDLE_DEMO}</span>
          <span title="Community supporter" style={{
            display: "inline-flex", alignItems: "center", gap: 4,
            fontSize: 10, fontWeight: 600, letterSpacing: "0.06em", textTransform: "uppercase",
            padding: "3px 8px", borderRadius: 999,
            background: accentColor, color: "#fff",
            boxShadow: `0 0 14px ${accentColor}88`,
          }}>
            <I.heart size={9} /> Supporter
          </span>
        </Row>
        {/* Name plate */}
        <div style={{
          display: "inline-block", alignSelf: "flex-start",
          padding: "4px 10px",
          background: `color-mix(in oklab, ${accentColor} 12%, transparent)`,
          border: `1px solid color-mix(in oklab, ${accentColor} 35%, transparent)`,
          borderRadius: "var(--r-sm)",
          fontFamily: "var(--font-mono)", fontSize: 12,
          color: "var(--fg)",
          letterSpacing: "0.02em",
        }}>
          ⟢ {plate || "Your name plate"}
        </div>
        <span style={{ fontSize: 12, color: "var(--fg-dim)", marginTop: 6 }}>
          {isMonthly
            ? <>Theme accent: <span className="mono" style={{ color: accentColor }}>{accentName}</span> · 5-year retention</>
            : <>Theme accent: <span className="mono">amber (default)</span> · 12-month retention</>}
        </span>
      </Stack>
    </Row>

    {/* Mini activity row mimicking profile look */}
    <hr className="ss-rule" style={{ margin: "18px 0 14px" }} />
    <Row gap={20} style={{ flexWrap: "nowrap" }}>
      <Stack gap={2}>
        <div className="ss-eyebrow">Sessions</div>
        <div className="mono" style={{ fontSize: 16, color: "var(--fg)" }}>184</div>
      </Stack>
      <Stack gap={2}>
        <div className="ss-eyebrow">Hours</div>
        <div className="mono" style={{ fontSize: 16, color: "var(--fg)" }}>312.7</div>
      </Stack>
      <Stack gap={2}>
        <div className="ss-eyebrow">Window</div>
        <div className="mono" style={{ fontSize: 13, color: isMonthly ? accentColor : "var(--fg-muted)" }}>
          {isMonthly ? "5 years" : "12 months"}
        </div>
      </Stack>
    </Row>
  </div>
);

const KeepRevertItem = ({ text, ok }) => (
  <Row gap={10} style={{ flexWrap: "nowrap", alignItems: "flex-start" }}>
    <span style={{
      width: 16, height: 16, borderRadius: 999,
      background: ok ? "color-mix(in oklab, var(--ok, #4ade80) 18%, transparent)" : "var(--bg-elev)",
      color: ok ? "var(--ok, #4ade80)" : "var(--fg-dim)",
      border: `1px solid ${ok ? "color-mix(in oklab, var(--ok, #4ade80) 35%, transparent)" : "var(--border)"}`,
      display: "grid", placeItems: "center", flexShrink: 0, marginTop: 2,
    }}>
      {ok ? <I.check size={9} /> : <I.arrow size={9} />}
    </span>
    <span style={{ fontSize: 13, color: "var(--fg-muted)", lineHeight: 1.5 }}>{text}</span>
  </Row>
);

window.Donate = Donate;
