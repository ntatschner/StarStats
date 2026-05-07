/**
 * StarStats — icons & small atoms
 * Lightweight stroke icons (1.5px) drawn from primitives, NOT
 * sci-fi-themed. They're functional UI affordances.
 */
const Icon = ({ d, size = 16, fill = false, sw = 1.6 }) => (
  <svg
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill={fill ? "currentColor" : "none"}
    stroke="currentColor"
    strokeWidth={sw}
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    {typeof d === "string" ? <path d={d} /> : d}
  </svg>
);

const I = {
  home:   (p) => <Icon {...p} d="M3 11.5 12 4l9 7.5V20a1 1 0 0 1-1 1h-5v-7H9v7H4a1 1 0 0 1-1-1Z" />,
  chart:  (p) => <Icon {...p} d="M4 20V8m6 12V4m6 16v-9m-12 9h16" />,
  device:(p) => <Icon {...p} d={<>
    <rect x="3" y="4" width="18" height="13" rx="2" />
    <path d="M8 21h8M12 17v4" />
  </>} />,
  user:   (p) => <Icon {...p} d={<>
    <circle cx="12" cy="9" r="3.5" />
    <path d="M5 20a7 7 0 0 1 14 0" />
  </>} />,
  key:    (p) => <Icon {...p} d={<>
    <circle cx="8" cy="14" r="3.5" />
    <path d="M11 12 21 4l-3 3 1.5 1.5L17 11l-2-2" />
  </>} />,
  globe:  (p) => <Icon {...p} d={<>
    <circle cx="12" cy="12" r="9" />
    <path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" />
  </>} />,
  cog:    (p) => <Icon {...p} d={<>
    <circle cx="12" cy="12" r="3" />
    <path d="M12 2v3M12 19v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M2 12h3M19 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1" />
  </>} />,
  bell:   (p) => <Icon {...p} d={<>
    <path d="M6 17V11a6 6 0 1 1 12 0v6l1.5 2H4.5L6 17Z" />
    <path d="M10 21h4" />
  </>} />,
  copy:   (p) => <Icon {...p} d={<>
    <rect x="8" y="8" width="12" height="12" rx="2" />
    <path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" />
  </>} />,
  eye:    (p) => <Icon {...p} d={<>
    <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12Z" />
    <circle cx="12" cy="12" r="3" />
  </>} />,
  eyeoff: (p) => <Icon {...p} d={<>
    <path d="m4 4 16 16" />
    <path d="M9.9 5.1A11 11 0 0 1 12 5c6.5 0 10 7 10 7a16 16 0 0 1-3.6 4.4M6.6 6.6A16 16 0 0 0 2 12s3.5 7 10 7a11 11 0 0 0 4.6-1" />
    <path d="M9.9 9.9a3 3 0 0 0 4.2 4.2" />
  </>} />,
  check:  (p) => <Icon {...p} d="m4 12 5 5L20 6" />,
  arrow:  (p) => <Icon {...p} d="m9 6 6 6-6 6" />,
  arrowup:(p) => <Icon {...p} d="m6 15 6-6 6 6" />,
  plus:   (p) => <Icon {...p} d="M12 5v14M5 12h14" />,
  x:      (p) => <Icon {...p} d="M5 5l14 14M19 5 5 19" />,
  zap:    (p) => <Icon {...p} d="M13 2 3 14h7l-1 8 10-12h-7l1-8Z" />,
  shield: (p) => <Icon {...p} d="M12 3 4 6v6c0 4.5 3.4 8.4 8 9 4.6-.6 8-4.5 8-9V6l-8-3Z" />,
  link:   (p) => <Icon {...p} d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 1 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 1 0 5.7 5.7l1-1" />,
  rocket: (p) => <Icon {...p} d={<>
    <path d="M12 2c4 3 6 7 6 12l-3 3-6-6 3-3a14 14 0 0 1 0-6Z" />
    <path d="M9 14c-3 1-5 4-5 7 3 0 6-2 7-5M14 9a1 1 0 1 0 0-.001Z" />
  </>} />,
  signal: (p) => <Icon {...p} d="M3 18h2V14H3v4Zm5 0h2V10H8v8Zm5 0h2V6h-2v12Zm5 0h2V2h-2v16Z" fill={true} sw={0} />,
  download:(p) => <Icon {...p} d="M12 4v12m0 0-4-4m4 4 4-4M4 20h16" />,
  trash:  (p) => <Icon {...p} d={<>
    <path d="M4 7h16M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2" />
    <path d="M6 7v13a1 1 0 0 0 1 1h10a1 1 0 0 0 1-1V7" />
    <path d="M10 11v6M14 11v6" />
  </>} />,
  github: (p) => <Icon {...p} sw={0} fill={true} d="M12 2a10 10 0 0 0-3.16 19.49c.5.09.68-.22.68-.48l-.01-1.7c-2.78.6-3.37-1.34-3.37-1.34-.45-1.16-1.11-1.47-1.11-1.47-.91-.62.07-.61.07-.61 1 .07 1.53 1.03 1.53 1.03.89 1.53 2.34 1.09 2.91.83.09-.65.35-1.09.63-1.34-2.22-.25-4.55-1.11-4.55-4.94 0-1.09.39-1.99 1.03-2.69-.1-.25-.45-1.27.1-2.65 0 0 .84-.27 2.75 1.02a9.56 9.56 0 0 1 5 0c1.91-1.29 2.75-1.02 2.75-1.02.55 1.38.2 2.4.1 2.65.64.7 1.03 1.6 1.03 2.69 0 3.84-2.34 4.69-4.57 4.93.36.31.68.92.68 1.85l-.01 2.74c0 .27.18.58.69.48A10 10 0 0 0 12 2Z" />,
  heart:  (p) => <Icon {...p} d="M12 20s-7-4.35-7-10a4 4 0 0 1 7-2.65A4 4 0 0 1 19 10c0 5.65-7 10-7 10Z" />,
};

window.I = I;
window.Icon = Icon;
