# Security Policy

StarStats is a hobby project run by a single maintainer. We take
security seriously but the response surface is one person, so set
your expectations accordingly. The disclosures below describe how to
report a problem, what's in scope, and which dependency advisories we
have already triaged and accepted.

## Reporting a vulnerability

**Please do not open a public issue for security problems.**

Preferred channel — **GitHub Security Advisories** (private):
<https://github.com/ntatschner/StarStats/security/advisories/new>.
This goes straight to the maintainer and is invisible until we
publish.

Fallback channel — email **dojo@thecodesaiyan.io**. PGP not yet
available; if you want an encrypted channel, mention it in your first
message and we can arrange one.

We aim to acknowledge within 7 days and to triage within 30. Critical
issues affecting authentication, account takeover, or unintended data
disclosure get prioritised over everything else, including ongoing
feature work.

## What's in scope

- The hosted server and the `apps/web` app.
- The Tauri desktop client and its updater channel.
- The Rust API endpoints under `/v1/*`, including auth, sharing,
  device pairing, and the OIDC discovery surface.
- The migrations and schema in `crates/starstats-server/migrations`.

## What's out of scope

- Volumetric DoS / DDoS (the project runs on a single host).
- Self-XSS, social-engineering tricks against the maintainer, or
  attacks that require a malicious extension already installed in the
  victim's browser.
- Third-party services we depend on (RSI's website, GitHub, the SMTP
  relay) — report those to their owners.
- Bugs in unreleased branches.

## Known accepted dependency advisories

The following Dependabot advisories have been triaged and accepted as
**non-exploitable in this codebase**. They remain dismissed in the
GitHub Dependabot panel with a `tolerable_risk` rationale; this
section restates the reasoning so it survives outside the GitHub UI.

All five rustls/aws/lru entries below clear automatically once
`aws-sdk-s3` can be bumped past `=1.110.0` — the workspace currently
pins it for MSRV reasons. The `glib` entry clears once Tauri bumps
its gtk dependency upstream.

| Advisory | Severity | Crate (transitive root) | Why accepted |
|---|---|---|---|
| [GHSA-82j2-j2ch-gfr8](https://github.com/advisories/GHSA-82j2-j2ch-gfr8) | High | `rustls-webpki 0.101.7` (via `aws-sdk-s3` → `rustls 0.21`) | Vulnerability requires opt-in CRL checking with attacker-controlled CRL bytes. The AWS SDK's S3 TLS path does not enable CRL checking, and we never pass `RevocationOptions` to a verifier; the bug is unreachable from our process. |
| [GHSA-xgp8-3hg3-c2mh](https://github.com/advisories/GHSA-xgp8-3hg3-c2mh) | Low | `rustls-webpki 0.101.7` (same root) | Name-constraint acceptance issue. We do not configure name constraints on the AWS SDK TLS path; the AWS SDK uses a default trust store with no explicit name-constraint extensions. |
| [GHSA-965h-392x-2mh5](https://github.com/advisories/GHSA-965h-392x-2mh5) | Low | `rustls-webpki 0.101.7` (same root) | URI-name-constraint variant of the above. Same reasoning: we do not configure URI name constraints. |
| [GHSA-g59m-gf8j-gjf5](https://github.com/advisories/GHSA-g59m-gf8j-gjf5) | Low | `aws-sdk-s3 1.110.0` (direct, pinned) | Defense-in-depth enhancement around the `region` parameter, not an exploit. Our region values come from server configuration we control; user input never reaches the AWS region parameter. |
| [GHSA-rhfx-m35p-ff5j](https://github.com/advisories/GHSA-rhfx-m35p-ff5j) | Low | `lru 0.12.5` (via `aws-sdk-s3`) | Stacked-Borrows soundness issue in `IterMut` reported under Miri. Not a runtime memory-safety bug. We never call `IterMut` on the lru caches AWS SDK uses internally. |
| [GHSA-wrw7-89jp-8q8g](https://github.com/advisories/GHSA-wrw7-89jp-8q8g) | Medium | `glib 0.18.5` (Linux only, via `tauri` → `tray-icon` → `libappindicator` → `gtk`) | Soundness issue in `VariantStrIter`. We do not call `VariantStrIter` from any of our code. The dep enters the tree only on Linux desktop builds and is wholly internal to the Tauri stack. |

If any of these *become* reachable in our codebase (for example, if
we add CRL checking, take user-controlled region input, or call
`VariantStrIter` directly), revisit the relevant row. Bumping
`aws-sdk-s3` past `=1.110.0` clears the first five in one shot.

## Security architecture (quick reference)

For deeper detail see [`apps/web/src/app/privacy/page.tsx`](apps/web/src/app/privacy/page.tsx)
§9, which describes what's customer-visible. In summary:

- Passwords: Argon2id.
- Sessions: HttpOnly + Secure cookies, 1-hour TTL.
- API tokens: RS256 JWT signed by an on-disk key generated at first
  boot.
- TOTP secrets: AES-256-GCM-encrypted with a file-based KEK
  (`totp-kek.bin`), fresh nonce per encrypt, never logged.
- Recovery codes: Argon2-hashed; we cannot read them back.
- Magic-link tokens: 15-minute, single-use, atomic
  `UPDATE … RETURNING` redeem so a token can't be replayed.
- Interim 2FA tokens: 5-minute, single-use, useless without the
  matching second factor.
- Anti-enumeration: magic-link start and password-reset start return
  the same response shape regardless of whether the email maps to an
  account, with timing equalised on the miss path.
