# My Bookshelf 4 — Security Review

**Date:** 18 October 2026  
**Original reviewer:** Astra (gpt-6-astra), via the GitHub connector  
**Validation update:** Codex, 18 October 2026, in the Rust-enabled environment  
**Project:** https://github.com/izderadicka/mbs4

## Scope and methodology

The review is pinned to:

- Backend: [`4c2313fef98640943061067cfe8529522b4bad5c`](https://github.com/izderadicka/mbs4/tree/4c2313fef98640943061067cfe8529522b4bad5c)
- Linked frontend: [`c12911803a8b0bb02ce399c7d2435b977f8cef01`](https://github.com/izderadicka/mbs4-client/tree/c12911803a8b0bb02ce399c7d2435b977f8cef01)

The original review was a source review. This update adds executed Rust and Chromium validation. The backend checkout matches the pinned commit above. The available frontend checkout is `227d9c922989c5c7e4da361a63076b0f06b1dee9`; its `src/lib/components/reader/reader.svelte` is identical to the originally reviewed version, and its installed Foliate dependency uses the same pinned commit `78914aef4466eb960965702401634c2cb348e9b1`.

Validation used Rust/Cargo 1.99.1 and Chromium 154.0.8037.57, temporary SQLite databases, loopback servers, synthetic users, and harmless EPUB markers. No live deployment was tested. Application source and tracked tests were not changed, and no issues, commits, or pull requests were created. The independent Rust harness calls the actual backend crates; all its dependency package versions/sources match packages in the backend lockfile. The browser harness exercises the actual pinned Foliate loader and renderer with the application's default security headers, rather than the full Svelte application.

The original source review reported coverage of backend source and frontend application, configuration, and test files, plus relevant dependency code: 154 backend text files and 209 frontend application/configuration/test files were retrieved. Generic frontend UI wrappers and binary assets were excluded from detailed review. It did not include a comprehensive vulnerability-advisory scan of all Rust, npm, Calibre, and LibreOffice dependencies.

## Findings overview

**The highest-priority finding is that a malicious EPUB can execute JavaScript with the application's origin when opened in the reader under the default configuration.** There are also two concrete authorization bypasses in batch conversions.

| # | Severity | Finding | Required conditions |
|---|---|---|---|
| 1 | High | JavaScript execution through EPUB reader | Victim opens a malicious EPUB |
| 2 | High, conditional | OIDC account takeover through unverified email linking | Provider permits attacker-controlled, unverified email claims |
| 3 | High, configuration-dependent | Credentialed API access from arbitrary origins | `--cors` enabled; browser sends authentication cookie |
| 4 | Medium | Private bookshelf disclosure through batch conversion | Attacker has trusted role |
| 5 | Medium | Batch ZIP download bypasses owner authorization | Any authenticated user |
| 6 | Medium | Login rate limiting bypass through forwarding headers | Headers reach application without trusted-proxy sanitization |
| 7 | Medium | Direct upload bypasses configured size limit | Attacker has trusted role |
| 8 | Medium | Stale sessions issue tokens with revoked privileges | Existing login session and valid retrieval-token window |
| 9 | Low | Post-login open redirect | Successful redirect-mode local login |

## Validation results added in this update

All nine findings are retained. Evidence levels differ; neither a successful general test suite nor the focused reproduction tests indicate that these issues are fixed. The focused tests assert the vulnerable behavior and will need to be inverted when fixes are implemented.

| Finding | Executed evidence | Remaining boundary |
|---|---|---|
| 1 — EPUB scripts | Reflowable and fixed-layout EPUBs changed the parent document and read a cookie-authenticated local response in Chromium; restrictive-CSP controls blocked both | Pinned Foliate integration harness, not full application UI or real API credentials |
| 2 — OIDC linking | Actual `UserClaim::try_from` accepts false/missing `email_verified`, preserving a different subject and the supplied email | No external provider attack or full OIDC callback flow; email-only lookup rechecked in source |
| 3 — CORS | Actual HTTP GET and POST preflight approve an arbitrary origin with credentials when enabled; disabled-mode control does not | Browser third-party-cookie delivery is deployment/browser-dependent and was not tested |
| 4 — Private shelf | Cross-user shelf read denied (403), but batch creation succeeds (200); private name and membership are disclosed | One-book shelf with no source; no conversion tool needed for disclosure |
| 5 — Batch ZIP | Ordinary non-owner gets metadata 403 but generated ZIP 200, including the private membership marker | Actual generated archive and manifest; no guessed production IDs |
| 6 — Login limiter | Same peer reaches 429 on attempt 11; rotating either forwarding header permits 11 further attempts per header | Direct local connection; trusted reverse-proxy configurations not evaluated |
| 7 — Upload limit | At a 1 MiB configured cap, 1,048,593-byte fixed-length and chunked uploads both return 201 | Small bounded payloads; disk exhaustion not attempted |
| 8 — Stale session | After role removal and then account deletion, token exchange returns 200 and fresh admin tokens access `/users` with 200 | Within the original session/retrieval-token window; no indefinite renewal claim |
| 9 — Redirect | Actual login returns 303 to both scheme-relative and slash/backslash external destinations, with a retrieval token | No external destination was contacted |

The workspace suite completed with **145 passed, 0 failed, 1 ignored, and 1 filtered out** using `cargo test --workspace --locked --offline -- --skip oidc::tests::test_discovery`. This includes all **31 backend HTTP integration tests**. The initial sandboxed attempt could not open sockets; the successful run used the environment's permitted execution mode. The excluded external OIDC discovery test subsequently passed in a separate permitted run (`cargo test --locked --offline -p mbs4-auth oidc::tests::test_discovery -- --nocapture`), bringing the combined baseline total to **146 passed and 1 existing ignored test**. The initial connection failure was environmental, not a product-test failure. Five focused HTTP tests and one focused OIDC claim test passed, covering all nine findings together with four browser cases.

## 1. High — EPUB scripts can run with the application's origin

The frontend passes downloaded books directly into Foliate using `await v.open(file)`, without installing a content-security transformation.

The pinned Foliate version preserves inline EPUB scripts, serializes book documents into blob URLs, and loads them in iframes configured with:

```html
sandbox="allow-same-origin allow-scripts"
```

The server's default CSP is:

```text
object-src 'none'; base-uri 'self'; frame-ancestors 'none'
```

There is no `script-src` or `default-src` restriction. Together, these settings allow scripts inside a book to execute in a same-origin frame, access the parent application, and make authenticated API requests.

An attacker could upload a malicious book and wait for an administrator to open it. A legitimate user importing a malicious externally obtained EPUB creates the same risk. Metadata extraction does not sanitize the original EPUB stored in the library.

`HttpOnly` cookies do not prevent this: scripts need not read the cookie to perform authenticated actions.

### Evidence

- [Application reader integration](https://github.com/izderadicka/mbs4-client/blob/c12911803a8b0bb02ce399c7d2435b977f8cef01/src/lib/components/reader/reader.svelte#L329-L360)
- [Default CSP](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-server/src/config.rs#L92-L98)
- [Pinned Foliate iframe sandbox](https://github.com/johnfactotum/foliate-js/blob/78914aef4466eb960965702401634c2cb348e9b1/paginator.js#L242-L244)
- [EPUB loader's script handling](https://github.com/johnfactotum/foliate-js/blob/78914aef4466eb960965702401634c2cb348e9b1/epub.js#L813-L863)
- [Upstream's explicit security warning](https://github.com/johnfactotum/foliate-js/blob/78914aef4466eb960965702401634c2cb348e9b1/README.md#L74-L84)

### Recommended fix

Enforce a reader-compatible CSP that blocks book scripts, including inline scripts and book-generated script blobs. Allow legitimate application bootstrap through hashes or nonces as necessary. A reader hosted on a separate origin would provide stronger isolation.

### Verification

Test both reflowable and fixed-layout EPUBs containing harmless scripts that attempt to modify `parent.document` or make an authenticated request. Neither must succeed. One harmless marker is:

```javascript
parent.document.documentElement.dataset.ebookScriptRan = "yes";
```

**Executed result:** Chromium reproduced parent-document modification and a same-origin request carrying an HttpOnly synthetic cookie for both reflowable and fixed-layout EPUBs. The harness also enabled `COOP: same-origin`, `COEP: require-corp`, and `CORP: same-origin`; `crossOriginIsolated` was true, so those headers do not prevent the demonstrated same-origin script access. Both cases were blocked by adding `script-src 'self'` to the default CSP. That control demonstrates blocking of these inline payloads, not a production-ready application CSP or comprehensive EPUB sanitization. Application bootstrap compatibility and book-generated external script blobs still require regression coverage when implementing a fix.

## 2. High, conditional — OIDC account linking does not verify email ownership

OIDC signature, state, nonce, and PKCE checks are present. The problem occurs afterward:

1. `UserClaim::try_from` accepts `claims.email()` without requiring `email_verified`.
2. The callback finds the local account by email alone.
3. The provider's stable subject is extracted but is not used to bind the local identity.

If a configured provider can issue a valid token containing a victim's email without verifying ownership, an attacker can authenticate as that existing local user—including an administrator.

This does not require forging a token. Conversely, it is not an unconditional takeover against every provider: a provider that reliably verifies these email claims mitigates this particular attack. The application itself does not enforce that assumption.

### Evidence

- [Email extraction](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-auth/src/oidc.rs#L197-L226)
- [Email-only account lookup](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-app/src/auth/oidc.rs#L266-L272)

### Recommended fix

Store external identities by `(issuer, subject)` and require an authenticated account-linking flow. If initial email-based linking remains, require verified email and an explicit trusted-provider policy. Email verification alone does not establish durable identity across providers or account changes.

### Verification

A valid provider token containing an existing administrator email with `email_verified=false` must not acquire that account. A different provider subject must not silently inherit an existing identity.

**Executed result:** A Rust unit harness constructed locally signed ID-token objects with `email_verified=false`, no `email_verified`, and `email_verified=true`. The application's `UserClaim::try_from` accepted all three and retained `admin@example.test` with `different-attacker-subject`. The callback source still calls `find_by_email` without binding issuer/subject. This validates the missing claim check; it does not demonstrate that any particular external provider will issue the required claims or that token signature checks can be bypassed.

## 3. High when enabled — permissive CORS exposes authenticated APIs

`CorsLayer::very_permissive()` accepts arbitrary request origins while allowing credentials. Enabling this mode also changes the authentication cookie to `SameSite=None`.

A malicious website can therefore make authenticated requests and read responses as the victim when the browser sends the cookie. For an administrator, that includes user-management operations.

This is **disabled by default**, and the configuration help says to leave it disabled in production. Browser third-party-cookie restrictions may block some attacks, but they do not make this origin policy safe; same-site, cross-origin scenarios also matter.

### Evidence

- [CORS configuration](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-server/src/run.rs#L65-L69)
- [Cookie configuration](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-app/src/auth/token.rs#L259-L274)
- [Matching tower-http 0.6.8 implementation](https://github.com/tower-rs/tower-http/blob/tower-http-0.6.8/tower-http/src/cors/mod.rs)

### Recommended fix

Accept only explicitly configured frontend origins. Development access should identify the intended localhost origin and port.

### Verification

An unlisted origin must receive no credentialed CORS approval, for both reads and state-changing requests.

**Executed result:** Against the real local server with CORS enabled, a GET to the administrator `/users` endpoint returned `Access-Control-Allow-Origin: https://untrusted.example` and `Access-Control-Allow-Credentials: true`. A POST preflight approved the same origin, method, and credentials. With CORS disabled, those approvals were absent. The protocol test used a bearer token for the GET; actual cross-site browser cookie delivery remains a condition, while the application's `SameSite=None; Secure; HttpOnly` cookie settings were confirmed in source.

## 4. Medium — batch conversion exposes another user's private bookshelf

The normal bookshelf endpoints enforce public/owner access. `POST /api/convert/batch` bypasses that authorization when the requested entity is a bookshelf.

It directly enumerates the shelf's ebook IDs and retrieves its metadata, then creates a batch owned by the requesting user. A trusted user can supply someone else's private shelf ID and learn its name and membership through their own batch metadata, progress, conversion records, and manifest.

Even failed conversions can disclose titles and IDs. Books above the 100-item conversion cap are listed by ID in the manifest's skipped section, up to the repository enumeration cap.

Individual books are globally readable in this application. The violation is exposure of the **private bookshelf's name and composition**.

### Evidence

- [Batch creation without shelf authorization](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-app/src/ebook_format/batch.rs#L125-L182)
- [Unscoped enumeration](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-dal/src/bookshelf.rs#L441-L459)
- [Manifest generation](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-app/src/ebook_format/convertor.rs#L735-L754)

### Recommended fix

Authorize access to the bookshelf before enumerating items or constructing the batch. Reuse the authorization logic from ordinary bookshelf reads.

### Verification

A trusted Alice must be denied when requesting conversion of Bob's private shelf. Bob and users allowed to access public shelves should retain the intended behavior.

**Executed result:** A trusted second user received 403 from a private bookshelf's normal read endpoint, then 200 from `POST /api/convert/batch` for that same shelf. Their batch disclosed `Private shelf marker`, and its generated manifest disclosed `Private membership marker | id=1` even though the book had no conversion source. The source enumeration cap is specifically **1,000 ebook IDs**; the first 100 are processed and the remaining enumerated IDs can appear as skipped. The over-cap branch was source-checked, not exercised in the focused test.

## 5. Medium — batch ZIP downloads bypass owner protection

Batch metadata endpoints check that the requester is the owner or an administrator. The archive download does not.

Archives have predictable paths:

```text
batches/batch-{batch_id}.zip
```

The generic conversion download endpoint serves them without checking batch ownership. Any authenticated user, including one without the trusted role, can request:

```http
GET /files/download/conversion/batches/batch-123.zip
```

This exposes another user's export and manifest by guessing batch IDs, despite being denied access to its metadata endpoint. Individual library books are otherwise shared; the sensitive information is the grouping/export and its manifest.

### Evidence

- [Owner/admin check](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-app/src/rest_api/conversion_batch.rs#L35-L53)
- [Predictable archive path](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-app/src/ebook_format/convertor.rs#L583-L595)
- [Generic download route and middleware placement](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-app/src/store/rest_api.rs#L299-L375)

### Recommended fix

Serve batch archives through an owner-authorized batch endpoint and prevent generic downloads from bypassing it. Random filenames alone would not fix the authorization problem.

### Verification

A second user must be denied both batch metadata and the corresponding ZIP.

**Executed result:** An ordinary non-owner received 403 from `/api/conversion-batch/{id}` but 200 from `/files/download/conversion/batches/batch-{id}.zip`. The downloaded archive was generated by the real batch worker, and its `manifest.txt` contained the private book title and ID. This confirms that the failure does not depend on the requesting user having the trusted role.

## 6. Medium — forwarding headers bypass login rate limiting

`client_ip` trusts the first `Forwarded` or `X-Forwarded-For` value regardless of the actual connecting peer. That value identifies the bucket for the ten-attempts-per-minute login limit.

An attacker can change the supplied address on every attempt and obtain a fresh bucket. This permits password guessing and repeated Argon2 work beyond the intended bound. It can also consume another address's allowance.

This matters when the application is directly reachable or the proxy does not sanitize both headers. The bundled Compose configuration publishes the application port. A correctly configured proxy plus restricted backend access can mitigate the issue.

### Evidence

- [Header trust](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-app/src/auth/mod.rs#L126-L138)
- [Limiter use](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-app/src/auth/mod.rs#L228-L232)
- [Limiter implementation](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-app/src/auth/rate_limit.rs#L10-L58)

### Recommended fix

Accept forwarding headers only from configured trusted proxies; otherwise use the socket peer address. Account-based throttling and a global password-verification concurrency bound would provide additional protection.

### Verification

Repeated failures from one peer must remain throttled when fake forwarding addresses change.

**Executed result:** Ten invalid login attempts from one loopback peer returned 401 and the eleventh returned 429. From that same peer, eleven requests each with different `X-Forwarded-For` addresses returned 401, followed by eleven using different `Forwarded` addresses that also returned 401. All addresses were documentation-range synthetic values; no real account guessing was performed.

## 7. Medium — direct uploads ignore the configured size limit

The upload router applies Axum's `DefaultBodyLimit`, but `upload_direct` accepts a raw `Request` and streams its body directly into storage.

`DefaultBodyLimit` only protects body consumption that uses the relevant extractor/limiting mechanism. This raw streaming path does not, and the storage loop has no independent byte cap.

A trusted user can therefore exceed `MBS4_UPLOAD_LIMIT_MB`—100 MiB by default—including with chunked requests, consuming disk beyond the configured limit.

### Evidence

- [Raw body consumption](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-app/src/store/rest_api.rs#L217-L244)
- [Router limit](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-app/src/store/rest_api.rs#L364-L375)
- [Matching Axum 0.8.8 implementation](https://github.com/tokio-rs/axum/blob/axum-v0.8.8/axum-core/src/extract/default_body_limit.rs)

### Recommended fix

Enforce the cap on the consumed stream, using `RequestBodyLimitLayer` or explicit body limiting. Clean up partial files after rejection.

### Verification

With a small configured limit, oversized uploads must return 413 both with and without `Content-Length`, leaving no temporary file.

**Executed result:** With `upload_limit_mb=1`, direct uploads of 1,048,593 bytes returned 201 and reported the entire payload size, both with Content-Length and with chunked transfer. A multipart upload of the same size was rejected, but returned **500**, not 413. That error mapping also needs correction. Partial-file cleanup after a rejected upload was not checked; the recommendation to test cleanup remains open.

## 8. Medium — stale sessions can issue fresh tokens after revocation

OIDC and redirect-mode local login store a full `User` snapshot, including roles, in the session. `/auth/token` issues a fresh JWT from that snapshot without reloading the account.

Deleting the user, resetting its password, or removing its administrator role does not update that stored snapshot. Within the valid session and retrieval-token window, the session can issue a **new token carrying revoked privileges**.

Existing JWTs also remain valid until expiry—one day by default. That is expected for self-contained JWTs without revocation. The additional defect here is issuing fresh tokens from stale account data.

The retrieval token lasts five minutes, is neither user/session-bound nor consumed on use. This finding does **not** establish indefinite token renewal: authentication-session and retrieval-token lifetimes still matter.

### Evidence

- [Session snapshot](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-app/src/auth/mod.rs#L148-L163)
- [Token issuance](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-app/src/auth/token.rs#L277-L300)
- [JWT validation](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-app/src/auth/token.rs#L165-L175)

### Recommended fix

Store a user identifier in the session and reload current account status and roles before issuance. Add a session/authentication version for revocation. Bind retrieval tokens to their login session and make them single-use.

### Verification

Revoke a role or delete the account between login and token exchange; the exchange must fail or reflect current privileges.

**Executed result:** After redirect-mode local login as a synthetic administrator, the database role was removed. Exchanging the original session cookie and retrieval token still returned a newly issued JWT containing `admin`, and that token accessed `/users` with 200. Deleting the user and repeating the same exchange produced the same result. The harness deliberately forwarded the session cookie over its loopback test connection; it did not rely on browser treatment of Secure cookies over HTTP. Reuse of the retrieval token was also demonstrated. Password-reset revocation and OIDC-session issuance were source-checked rather than separately reproduced.

## 9. Low — post-login redirect validation accepts external destinations

The redirect guard accepts values starting with `/` if they do not contain `://`. Consequently, this passes:

```text
//attacker.example/collect
```

URL joining resolves it to the attacker's origin, and the application appends the `trt` retrieval token.

The original URL-behavior harness, now corroborated by real local-login HTTP tests, reproduced the bypass for both `//attacker.example/collect` and a slash/backslash variant (`/\attacker.example/collect`). This enables phishing and leaks the retrieval token. That token alone does not authenticate an attacker: exchanging it also requires an authenticated session.

### Evidence

- [Redirect validation and token append](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-app/src/auth/mod.rs#L165-L185)
- [URL joining](https://github.com/izderadicka/mbs4/blob/4c2313fef98640943061067cfe8529522b4bad5c/crates/mbs4-app/src/state.rs#L98-L103)

### Recommended fix

Resolve the destination and require its origin to equal the configured frontend origin. Reject authority-form paths and backslashes as well.

### Verification

Test scheme-relative destinations, backslash variants, ordinary local paths, and legitimate configured frontend destinations. External origins must be rejected before adding the retrieval token.

**Executed result:** Real redirect-mode login returned 303 and an external `Location` for both `//attacker.example/collect` and `/\attacker.example/collect`; both carried a `trt` query parameter. The ordinary `/` control stayed on localhost. The test disabled redirect following and never contacted the external host. This replaces the original URL-only harness evidence with application-level behavior.

## Other reviewed areas

The following are retained source-review observations, not claims of exhaustive dynamic verification:

- SQL values use binds, and dynamic ordering uses field allowlists. No SQL injection was confirmed.
- Ordinary private-bookshelf endpoints, administrator routes, and per-user SSE filtering have appropriate checks. The batch paths above are the exceptions.
- Storage paths reject absolute paths, dot segments, backslashes, colons, and control characters. No straightforward remote path traversal was found. Symlinks in administrator-managed storage remain a filesystem trust consideration.
- OIDC includes state, nonce, PKCE, signature verification, and restricted configured-provider discovery.
- Passwords use Argon2 with random salts. JWT secrets are randomly generated and created with restrictive Unix permissions.
- Calibre and LibreOffice receive argument arrays rather than shell commands, with execution timeouts and conversion concurrency limits. No shell injection was found.
- Svelte escapes ebook descriptions. The application's identified `{@html}` usage renders configured login text rather than ordinary ebook metadata.
- No separate signed-download-URL mechanism exists in this reviewed snapshot; downloads use authenticated requests. The HMAC-based retrieval-token implementation checks length, signature, and expiry.

Conversion processes still parse complex untrusted documents with the server's filesystem privileges. Isolating them and imposing memory, disk, and network limits would strengthen containment.

Pagination accepts `page=0` and performs unchecked `u32` arithmetic. This can panic request tasks in debug builds or wrap in release builds. It deserves correction, but the review did not establish process-wide denial of service, so it was not elevated to a principal finding.

## Recommended remediation order

1. Fix EPUB script isolation.
2. Close the batch authorization gaps and correct OIDC identity linking.
3. Keep permissive CORS disabled; replace it with explicit origin configuration where needed.
4. Correct session reissuance/revocation, login throttling, and streaming upload limits.
5. Fix redirect validation and pagination arithmetic.

## Validation limitations

This update adds local reproduction evidence; it is not a live penetration test, a remediation patch, or a full repeat of the original broad source audit. OIDC takeover remains conditional on provider behavior and was not exercised through an external provider. CORS approval was tested over HTTP, while real browser cross-site cookie behavior was not. EPUB execution was tested with the pinned Foliate dependency in a minimal reader harness using matching default security headers and a synthetic cookie, not through the full Svelte application. Other browsers, application CSP compatibility, external-script EPUB payloads, full disk exhaustion, and deployment-specific proxy controls were not tested.

The baseline suite has an existing ignored test, and external discovery results are separate. No comprehensive dependency vulnerability-advisory scan was performed. Statements about SQL injection, shell injection, storage traversal, and pagination remain bounded source-review observations. A passing reproduction test means the reported vulnerable behavior was observed, not fixed.

## Reproduction artifacts

The companion `mbs4-security-validation-evidence.zip` includes the independent Rust harness and lockfile, harmless EPUB generator, browser harness, and test logs. Its README documents commands, assumptions, and expected vulnerable outcomes. All fixtures use temporary local data. Backend and frontend tracked worktrees were clean after validation.
