# IDaaS — Technical Reference

This document is the authoritative technical reference for the IDaaS (Identity-as-a-Service) platform. It describes what is built, how it is structured, and the key implementation decisions. Keep it current as the codebase evolves.

---

## Project Overview

A self-hosted, Auth0-alternative identity platform written in Rust. Provides multi-tenant authentication (database, OAuth2, OIDC, SAML), RS256 JWT issuance with webhook enrichment, regional data residency, and a 5-phase crash-safe geographic migration protocol. Managed via a REST API and a React back office (`idass-admin`).

**Key differentiators vs. Auth0:**
- Fixed infrastructure cost — no per-MAU pricing
- Regional data residency built in (not an Enterprise add-on)
- Webhook enrichment in-process with a hard 150 ms timeout (no cold-start latency)
- Full control over rate limits

---

## Architecture

### Modular Monolith (Cargo Workspace)

Five crates compile to one binary. Modules communicate via `Arc<T>` in memory — zero inter-module network overhead. The port-trait abstraction in `api-gateway` decouples HTTP handlers from domain logic for testability.

```
api-gateway        ← Axum HTTP server; port traits; all route handlers
├── mod-auth       ← Argon2id DB auth · Google OAuth2 · OIDC · SAML 2.0
├── mod-token      ← RS256 JWT minting · DashMap key cache · webhook enrichment
├── mod-mgmt       ← Tenant/user/org/connection CRUD · geographic migration
└── shared-kernel  ← Domain models · AppError→HTTP · AES-GCM-256 crypto
```

### Regional Data Residency

Each tenant is pinned to one PostgreSQL instance per region. `api-gateway` holds a `HashMap<String, PgPool>` — one entry per region. Every query is routed to the tenant's regional pool via the `RealMgmtService::pool_for_tenant` helper.

```
api-gateway
  AppState { auth, token, mgmt, csrf }   ← port trait objects
       │
  ┌────┴─────────┬──────────────────────┐
  ▼              ▼                      ▼
postgres-eu   postgres-us            redis
eu-west-1     us-east-1          (CSRF state)
```

### Port Trait Abstraction

`api-gateway/src/ports.rs` defines four traits:

| Trait | Responsibility |
|---|---|
| `AuthPort` | authenticate_db, generate_oauth_redirect, exchange_code_oauth, handle_saml_acs |
| `TokenPort` | mint (RS256 JWT with optional webhook enrichment) |
| `MgmtPort` | tenant/user/org/connection CRUD + migration + pool resolution |
| `CsrfStore` | OAuth2/OIDC CSRF state store/validate_and_consume |

`AppState` holds `Arc<dyn Trait>` for each. Production wires `RealAuthService`, `RealTokenService`, `RealMgmtService`, `RedisCsrfStore`. Tests use `mockall`-generated `MockAuthPort` etc. — no database required for handler tests.

---

## Repository Structure

```
idass/                            ← Rust backend (this repo)
├── Cargo.toml                    ← Workspace root; shared deps
├── rust-toolchain.toml           ← Pins Rust 1.88.0
├── .cargo/config.toml            ← SQLX_OFFLINE=true by default
├── .sqlx/                        ← Offline query cache (committed)
├── openapi.yaml                  ← OpenAPI 3.1 spec — authoritative contract
├── migrations/001_initial.sql    ← All DDL
├── docker-compose.yml            ← postgres-eu, postgres-us, redis, api-gateway
├── docker-compose.test.yml       ← Isolated test databases (ports 5434/5435)
├── Dockerfile                    ← Multi-stage Rust build → debian:bookworm-slim
├── shared-kernel/src/
│   ├── models.rs                 ← Domain structs (Tenant, User, Connection, …)
│   ├── error.rs                  ← AppError → HTTP status mapping
│   └── crypto.rs                 ← AES-GCM-256 encrypt/decrypt
├── mod-auth/src/
│   ├── db_auth.rs                ← Argon2id password verification
│   ├── oauth_google.rs           ← Google OAuth2 + JIT provisioning
│   ├── oidc.rs                   ← OIDC: discovery, redirect, token validation
│   └── saml_enterprise.rs        ← SAML AuthnRequest + ACS (sig validation stub)
├── mod-token/src/
│   ├── lib.rs                    ← TokenService: RS256 JWT + DashMap key cache
│   └── webhooks.rs               ← Claim enrichment with 150 ms timeout
├── mod-mgmt/src/
│   ├── tenant_crud.rs
│   ├── user_crud.rs
│   ├── connection_crud.rs
│   ├── org_crud.rs
│   └── migration.rs              ← 5-phase geographic migration state machine
└── api-gateway/src/
    ├── main.rs                   ← AppState + startup wiring
    ├── ports.rs                  ← AuthPort, TokenPort, MgmtPort, CsrfStore traits
    ├── services.rs               ← RealAuthService, RealTokenService, RealMgmtService
    ├── csrf.rs                   ← RedisCsrfStore + InMemoryCsrfStore (tests)
    ├── router.rs                 ← All route handlers + middleware
    └── lib.rs                    ← Re-exports for integration tests

idass-admin/                      ← React back office (separate repo)
```

---

## Data Model

All tables are identical in every regional database. Schema: `migrations/001_initial.sql`.

**Key tables:**

| Table | Purpose |
|---|---|
| `tenants` | One row per tenant; `status` = active / read_only / migrating |
| `connections` | Identity provider configs; `strategy` = database / oauth2 / saml / oidc; `options_encrypted BYTEA` |
| `users` | Core PII; linked to tenant + connection; `external_provider_id` = OAuth2/OIDC `sub` or SAML NameID |
| `organizations` | B2B org groupings within a tenant |
| `tenant_keys` | Encrypted RS256 private keys for JWT signing |
| `migration_outbox` | Transactional outbox for crash-safe geo-migration |

All queries in domain crates **must include `WHERE tenant_id = $1`** on every read/write. No cross-tenant queries exist.

Connection `options_encrypted` stores a JSON blob encrypted with AES-GCM-256. The `MASTER_ENCRYPTION_KEY` env var (32 bytes, base64) is the only secret that must be protected externally. The plaintext JSON structure varies by strategy — see the Authentication Strategies section.

---

## Authentication Strategies

### Database (`strategy = "database"`)

Argon2id password hashing (PHC string format). Verification runs in `tokio::task::spawn_blocking` to avoid blocking the async runtime.

`options_encrypted` stores nothing extra for this strategy — credentials are on the user row.

### OAuth2 Google (`strategy = "oauth2"`)

Options stored in `options_encrypted`:
```json
{ "client_id": "…", "client_secret": "…", "redirect_uri": "…", "token_endpoint": "…" }
```

CSRF state stored in Redis (300 s TTL, atomic `GETDEL`). Callback validates cookie + query param via constant-time compare. New users JIT-provisioned on first login. ID token decoded without signature verification (MVP limitation — use OIDC strategy for production providers).

### OpenID Connect (`strategy = "oidc"`)

Standards-compliant OIDC supporting any provider (Keycloak, Okta, Azure AD, Dex, etc.).

**At connection creation**, the handler calls `mod_auth::oidc::resolve_oidc_config` which either:
- `mode = "discover"`: fetches `{issuer_url}/.well-known/openid-configuration` — validated against SSRF (must be HTTPS, globally routable IP, no redirects)
- `mode = "custom"`: uses the provided `discovery_document` JSON directly

The **resolved endpoints** are stored in `options_encrypted`:
```json
{ "issuer": "…", "authorization_endpoint": "…", "token_endpoint": "…", "jwks_uri": "…",
  "client_id": "…", "client_secret": "…", "redirect_uri": "…" }
```

**At callback**, JWKS is fetched from the stored `jwks_uri` for ID token signature validation (`openidconnect::CoreIdTokenVerifier::new_confidential_client`). The RS256 signature, `iss`, `aud`, and `exp` are all verified.

**SSRF protection** in `resolve_oidc_config`: requires HTTPS, DNS-resolves the host, rejects loopback/private/link-local/ULA addresses, and disables redirect following.

### SAML 2.0 (`strategy = "saml"`)

Options stored in `options_encrypted`:
```json
{ "idp_sso_url": "…", "idp_entity_id": "…", "idp_certificate_pem": "…",
  "sp_entity_id": "…", "acs_url": "…" }
```

`generate_authn_request` produces a Deflate+Base64+URL-encoded `AuthnRequest` per the HTTP-Redirect binding spec.

⚠️ `handle_acs_response` is structurally complete but returns `400 InvalidConnection` until XML-DSIG signature validation is integrated. Do not enable SAML connections in production.

---

## Token Minting

RS256 JWTs. Each tenant has an encrypted RS256 key pair in `tenant_keys`. `TokenService` caches decoded `EncodingKey` values in `DashMap<Uuid, EncodingKey>` — DB reads only on first mint per tenant per process.

If a connection has `webhook_url`, a POST is sent with current claims before signing. The response JSON is merged. Hard 150 ms timeout — on error/timeout, the token is issued with base claims (fail-open; login is never blocked).

JWT claims: `sub` (user UUID), `email`, `tenant_id`, `org_id`, `iss`, `iat`, `exp`.

---

## Geographic Migration (5 phases)

| Phase | Action |
|---|---|
| 1 — Lock | CAS: `active → read_only`; concurrent requests get 503 immediately |
| 2 — Export | Read all entities in a source transaction; write `migration_started` outbox event |
| 3 — Import | Bulk-insert to target with `ON CONFLICT DO NOTHING` idempotency |
| 4 — Switch | Commit target first; write `migration_committed` recovery marker; activate at new region |
| 5 — GDPR Purge | Delete source data in FK-safe order; mark outbox `processed = TRUE` |

The `migration_committed` outbox event enables recovery: a worker polling `WHERE processed = FALSE` can re-run the purge if the process crashes between phases 4 and 5.

---

## API Structure

All management routes require `Authorization: Bearer <MANAGEMENT_API_KEY>` (constant-time comparison via `subtle::ConstantTimeEq`).

CORS is restricted to the `ADMIN_ORIGIN` env var (default `http://localhost:5173`). Redirect following is disabled. Methods limited to GET/POST/PUT/DELETE.

Full spec: `openapi.yaml` (OpenAPI 3.1). The `all_routes_are_documented_in_spec` contract test fails CI when a route is added without updating the spec.

Auth routes: `POST /:tenant/auth/database`, `GET /:tenant/auth/:conn/authorize`, `GET /:tenant/auth/:conn/callback`, `POST /:tenant/auth/saml/:conn/acs`

Management routes: tenant CRUD + migrate, user CRUD + list, connection CRUD + list, organization CRUD + list

---

## Testing

### Workspace test suites

| Suite | Count | Infrastructure |
|---|---|---|
| `shared-kernel` unit | 8 | None |
| `mod-auth` integration | 15 | PostgreSQL + WireMock |
| `mod-token` integration | 6 | PostgreSQL + WireMock |
| `mod-mgmt` integration | 16 | PostgreSQL |
| `api-gateway` handler tests | 25 | None (mockall mocks) |
| `api-gateway` contract tests | 15 | None (mockall mocks) |
| `api-gateway` integration | 2 | PostgreSQL (2 instances) |
| `mod-mgmt` migration | 1 `[ignore]` | PostgreSQL (2 instances) |

Run the suite:
```bash
DATABASE_URL=postgres://idass:idass@localhost:5433/idass_us_east_1 \
TEST_DATABASE_URL=postgres://idass:idass@localhost:5433/idass_us_east_1 \
  cargo test --workspace
```

**Note:** Port 5432 may conflict with a local Homebrew PostgreSQL. Use port 5433 (Docker postgres-us) to avoid this.

### Handler tests (no database required)

`api-gateway/tests/handler_tests.rs` — 25 tests using `mockall`-generated mocks. No Docker needed. Tests all 11 routes and key error scenarios.

### Contract tests (no database required)

`api-gateway/tests/contract_tests.rs` — validates every response body against `openapi.yaml` schemas using `jsonschema`. Includes `all_routes_are_documented_in_spec` drift detection.

### E2E tests (idass-admin)

`idass-admin/e2e/` — Playwright tests (19) against the live stack. Cover auth, tenant/user/connection/org management flows.

---

## Back Office Admin UI (`idass-admin`)

Separate React SPA repository. Vite + React 18 + TypeScript + shadcn/ui + TanStack Query + Axios.

Authenticates with `MANAGEMENT_API_KEY` (stored in `sessionStorage`). Fixed sidebar layout with tenant context. Supports all connection strategies including OIDC with discover/custom document toggle.

See `idass-admin/README.md` and `docs/superpowers/specs/2026-05-29-backoffice-ui-design.md`.

---

## Security Decisions

| Area | Decision |
|---|---|
| Connection secrets | AES-GCM-256 in `options_encrypted BYTEA`. Master key from env only. |
| Password hashing | Argon2id (PHC format). Verification in `spawn_blocking`. |
| JWT signing | RS256 per-tenant key pair. Private key encrypted at rest. DashMap cache. |
| Management API | `Authorization: Bearer` with `subtle::ConstantTimeEq` to prevent timing attacks. |
| OAuth2 CSRF | Redis `GETDEL` (atomic). Cookie + query param constant-time compare. |
| OIDC SSRF | HTTPS required. DNS-resolved IPs validated against private/loopback ranges. Redirects disabled. `token_endpoint` and `jwks_uri` also validated at creation time. |
| CORS | Explicit origin allowlist via `ADMIN_ORIGIN` env var. No wildcards. |
| Multi-tenancy | Every SQL query in domain crates carries `WHERE tenant_id = $1`. |
| SAML | ACS endpoint returns 400 until XML-DSIG is integrated — not safe for production. |

---

## Environment Variables

| Variable | Required | Description |
|---|---|---|
| `MASTER_ENCRYPTION_KEY` | Yes | Base64-encoded 32-byte AES-GCM key |
| `MANAGEMENT_API_KEY` | Yes | Bearer token for all mgmt endpoints |
| `DATABASE_URL` | Yes (compile) | Default DB; used by sqlx macro checks |
| `DATABASE_URL_EU_WEST_1` | Yes | PostgreSQL URL for eu-west-1 region |
| `DATABASE_URL_US_EAST_1` | No | PostgreSQL URL for us-east-1 region |
| `REGIONS` | No | Comma-separated list (default: `eu-west-1`) |
| `REDIS_URL` | No | Redis URL (default: `redis://127.0.0.1/`) |
| `ISSUER` | No | JWT `iss` claim (default: `https://idass.example.com`) |
| `APP_PORT` | No | HTTP bind port (default: `8080`) |
| `ADMIN_ORIGIN` | No | Allowed CORS origin for admin SPA (default: `http://localhost:5173`) |
| `RUST_LOG` | No | Log filter (recommended: `info,sqlx=warn`) |

---

## Code Quality Rules

1. **No compilation errors** — `cargo check --workspace` must pass clean.
2. **Secrets never in plaintext** — `client_secret`, private keys, certificates stored only in `options_encrypted` or `private_key_encrypted`, encrypted with AES-GCM-256.
3. **Multi-tenancy** — Every SQL query in domain crates must include `WHERE tenant_id = $1`.
4. **All code and comments in English.**
5. **Regenerate sqlx cache** after adding any `sqlx::query!` macro: `SQLX_OFFLINE=false DATABASE_URL=... cargo sqlx prepare --workspace -- --tests`
6. **Update `openapi.yaml`** when adding or changing any API endpoint.
