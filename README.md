# IDaaS — Identity as a Service

A production-ready MVP of a self-hosted identity platform built in Rust — an Auth0 alternative with multi-tenancy, regional data residency, and extensible token enrichment.

## Why IDaaS?

| Pain point | How IDaaS addresses it |
|---|---|
| Auth0's MAU-based pricing spikes at scale | Fixed infrastructure cost; no per-user fees |
| Rigid rate limits causing `HTTP 429` bursts | You control the rate limits |
| Cold-start latency from serverless "Actions" | Webhook enrichment runs in-process (150 ms hard timeout) |
| GDPR data residency only on Enterprise plans | Per-tenant regional PostgreSQL pools, built-in 5-phase geo-migration |

---

## Architecture

A **modular monolith** — five Cargo crates compiled into one binary. Modules communicate via `Arc<T>` in memory with zero network overhead, and can be split into separate services independently when needed.

```
api-gateway        ← Axum HTTP entry point; port-trait abstraction over all domain logic
├── mod-auth       ← Argon2id DB auth · Google OAuth2 · SAML 2.0 (ACS stub)
├── mod-token      ← RS256 JWT minting · DashMap key cache · webhook enrichment
├── mod-mgmt       ← Tenant/user/org CRUD · 5-phase geographic migration
└── shared-kernel  ← Domain models · AppError → HTTP · AES-GCM-256 crypto
```

### Regional Data Residency

Each tenant is pinned to a PostgreSQL instance in a specific region. `api-gateway` maintains a `HashMap<region, PgPool>` and routes every query to the correct pool. Moving a tenant to another region runs automatically with GDPR-compliant source purge.

```
┌─────────────────────────────────────────────────────────────┐
│                       api-gateway                           │
│   AppState { auth, token, mgmt, csrf }  ← port traits      │
└─────────────┬────────────┬────────────────────────┬─────────┘
              │            │                        │
     postgres-eu       postgres-us              redis
   eu-west-1:5432    us-east-1:5433           :6379
```

---

## Quick Start

**Prerequisite:** Docker + Docker Compose. That's it.

### 1. Clone and generate secrets

```bash
git clone <repo>
cd idass
cp .env.example .env

# Fill in the two required secrets (one-liners)
echo "MASTER_ENCRYPTION_KEY=$(openssl rand -base64 32)" >> .env
echo "MANAGEMENT_API_KEY=$(openssl rand -hex 32)"       >> .env
```

### 2. One command to start everything

```bash
docker compose up
```

Docker Compose will:
1. Pull and start **postgres-eu** (`:5432`), **postgres-us** (`:5433`), **redis** (`:6379`)
2. Build the `idass` binary from source (first run ~60 s; subsequent runs use the layer cache)
3. Wait for all three services to pass their healthchecks
4. Start **api-gateway** on **`http://localhost:8080`**
5. Run database migrations automatically on both regional pools at startup

Add `-d` to run in the background: `docker compose up -d`

### 3. Verify it's running

```bash
export MGMT_KEY=$(grep MANAGEMENT_API_KEY .env | cut -d= -f2)

# List tenants (empty array on first run)
curl -s http://localhost:8080/api/v1/mgmt/tenants \
  -H "Authorization: Bearer $MGMT_KEY" | jq .

# Create your first tenant
curl -s -X POST http://localhost:8080/api/v1/mgmt/tenants \
  -H "Authorization: Bearer $MGMT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name": "acme", "region": "eu-west-1"}' | jq .
```

### Development without Docker

If you prefer to run the binary directly (e.g. with `cargo watch`):

```bash
# Prerequisites: Rust 1.87, a running PostgreSQL, Redis
# rustup activates 1.87 automatically via rust-toolchain.toml

docker compose up -d postgres-eu postgres-us redis   # infra only
source .env
cargo run -p api-gateway
```

> **Note:** If you add a new `sqlx::query!` macro, regenerate the offline cache (the `-- --tests` flag includes queries inside `#[cfg(test)]` blocks):
> ```bash
> SQLX_OFFLINE=false DATABASE_URL=postgres://idass:idass@localhost:5432/idass_eu_west_1 \
>   cargo sqlx prepare --workspace -- --tests
> git add .sqlx && git commit -m "chore: update sqlx offline query cache"
> ```

---

## API Reference

Full spec: [`openapi.yaml`](./openapi.yaml) (OpenAPI 3.1)

### Authentication endpoints

All tenant-scoped. Replace `:tenant` with the tenant's unique name.

| Method | Path | Description |
|---|---|---|
| `POST` | `/api/v1/:tenant/auth/database` | Email/password login |
| `GET` | `/api/v1/:tenant/auth/:connection/authorize` | Begin OAuth2 flow (redirects to Google) |
| `GET` | `/api/v1/:tenant/auth/:connection/callback` | Complete OAuth2 flow, returns JWT |
| `POST` | `/api/v1/:tenant/auth/saml/:connection/acs` | SAML ACS endpoint *(stub — XML-DSIG pending)* |

**Login example:**
```bash
curl -s -X POST http://localhost:8080/api/v1/acme/auth/database \
  -H "Content-Type: application/json" \
  -d '{"email": "user@acme.com", "password": "hunter2"}' | jq .
# { "access_token": "eyJ...", "token_type": "Bearer" }
```

### Management endpoints

All require `Authorization: Bearer <MANAGEMENT_API_KEY>`.

**Tenant management**

| Method | Path | Description |
|---|---|---|
| `POST` | `/api/v1/mgmt/tenants` | Create tenant |
| `GET` | `/api/v1/mgmt/tenants` | List all tenants |
| `GET` | `/api/v1/mgmt/tenants/:id` | Get tenant by UUID |
| `PUT` | `/api/v1/mgmt/tenants/:id` | Update tenant name |
| `POST` | `/api/v1/mgmt/tenants/:id/migrate` | Trigger geographic migration |

**User management**

| Method | Path | Description |
|---|---|---|
| `POST` | `/api/v1/mgmt/:tenant/users` | Create user |
| `GET` | `/api/v1/mgmt/:tenant/users` | List users for tenant |
| `GET` | `/api/v1/mgmt/:tenant/users/:user_id` | Get user by UUID |
| `DELETE` | `/api/v1/mgmt/:tenant/users/:user_id` | Delete user |

**Connection management**

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/mgmt/:tenant/connections` | List connections for tenant |
| `POST` | `/api/v1/mgmt/:tenant/connections` | Create connection — strategy: database / oauth2 / saml / oidc |
| `PUT` | `/api/v1/mgmt/:tenant/connections/:id` | Update connection |
| `DELETE` | `/api/v1/mgmt/:tenant/connections/:id` | Delete connection |

**Organization management**

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/v1/mgmt/:tenant/organizations` | List organizations for tenant |
| `POST` | `/api/v1/mgmt/:tenant/organizations` | Create organization |
| `DELETE` | `/api/v1/mgmt/:tenant/organizations/:id` | Delete organization |

**Create tenant + connection + first user:**
```bash
MGMT_KEY=<your-key>

# Create tenant in eu-west-1
curl -s -X POST http://localhost:8080/api/v1/mgmt/tenants \
  -H "Authorization: Bearer $MGMT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name": "acme", "region": "eu-west-1"}' | jq .

# Create a database connection
curl -s -X POST http://localhost:8080/api/v1/mgmt/acme/connections \
  -H "Authorization: Bearer $MGMT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name": "default-db", "strategy": "database", "options": {}}' | jq .

# Create a user
curl -s -X POST http://localhost:8080/api/v1/mgmt/acme/users \
  -H "Authorization: Bearer $MGMT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"connection_id": "<conn-uuid>", "email": "user@acme.com", "password": "hunter2"}' | jq .
```

---

## Authentication Strategies

### Database (username/password)

Passwords hashed with **Argon2id** (PHC string format). Verification runs in a `spawn_blocking` thread pool to avoid blocking the async runtime.

### Google OAuth2

Configured per connection. Each connection stores encrypted `client_id`, `client_secret`, `redirect_uri`, and `token_endpoint` (AES-GCM-256 in `options_encrypted`). CSRF state is stored in Redis (atomic `GETDEL` on callback). New users are **JIT-provisioned** on first login.

### OpenID Connect (OIDC)

Standards-compliant OIDC strategy supporting any OIDC provider (Keycloak, Okta, Azure AD, Dex, etc.).

At **connection creation**, the server resolves the provider configuration once and stores the fully resolved endpoints in `options_encrypted`. Two input modes:

- `mode = "discover"` — provide `issuer_url`; server fetches `{issuer_url}/.well-known/openid-configuration`
- `mode = "custom"` — provide a `discovery_document` JSON blob directly

```bash
# Create an OIDC connection (auto-discover mode)
curl -s -X POST http://localhost:8080/api/v1/mgmt/acme/connections \
  -H "Authorization: Bearer $MGMT_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "keycloak",
    "strategy": "oidc",
    "options": {
      "mode": "discover",
      "issuer_url": "https://keycloak.example.com/realms/acme",
      "client_id": "my-app",
      "client_secret": "s3cr3t",
      "redirect_uri": "https://app.example.com/callback"
    }
  }' | jq .
```

At **authorize/callback time**, the stored endpoints are used directly — no discovery HTTP call on the hot path. JWKS is fetched at callback time for RS256 ID token signature validation.

**SSRF protection:** `issuer_url` and all resolved endpoints must be HTTPS and resolve to globally routable IP addresses (loopback, RFC1918, link-local/169.254.x.x, and IPv6 ULA/link-local are all rejected). Redirect following is disabled.

### SAML 2.0

`generate_authn_request` produces a Deflate-compressed, Base64-encoded, URL-encoded `AuthnRequest` per the HTTP-Redirect binding spec. The ACS endpoint (`handle_acs_response`) is structurally complete but XML-DSIG signature validation is not yet integrated — it returns `400 InvalidConnection` until an XML-DSIG library is wired in.

---

## Token Minting

Tokens are signed **RS256** JWTs. Each tenant has a key pair stored encrypted (`private_key_encrypted BYTEA`) in the `tenant_keys` table. The `TokenService` caches decoded `EncodingKey` values in a `DashMap<Uuid, EncodingKey>` — DB reads only on first mint per tenant per process.

**Claims:**
```json
{
  "sub": "<user-uuid>",
  "email": "user@example.com",
  "tenant_id": "<tenant-uuid>",
  "org_id": "<org-uuid>",
  "iss": "https://idass.example.com",
  "iat": 1234567890,
  "exp": 1234571490
}
```

### Webhook Enrichment

If a connection has a `webhook_url`, a `POST` with current claims is sent before signing. The response JSON is merged into the token. A **150 ms** hard timeout applies — on any error or timeout, the token is issued with the base claims (fail-open, no login blocked).

---

## Geographic Migration

Moves a tenant between regional databases in 5 atomic phases. Safe against crashes at any point:

| Phase | Action |
|---|---|
| 1 — Lock | CAS transition: `active → read_only`. In-flight requests receive `503`. |
| 2 — Export | Read orgs, connections, users, keys inside a source transaction; write `migration_started` outbox event. |
| 3 — Import | Bulk insert to target with `ON CONFLICT DO NOTHING` idempotency guards. |
| 4 — Switch | Commit target first; write `migration_committed` outbox to target (recovery marker); activate tenant at new region. |
| 5 — GDPR Purge | Delete source data in FK-safe order; mark source outbox `processed = TRUE`. |

A background worker polling `migration_committed WHERE processed = FALSE` can re-run the purge if the process crashes between phases 4 and 5.

**Trigger migration:**
```bash
curl -s -X POST http://localhost:8080/api/v1/mgmt/tenants/<uuid>/migrate \
  -H "Authorization: Bearer $MGMT_KEY" \
  -H "Content-Type: application/json" \
  -d '{"target_region": "us-east-1"}'
# 204 No Content on success
```

---

## Running Tests

### Unit + integration tests (domain crates)

Requires Docker services running. `#[sqlx::test]` creates isolated test databases per test.

```bash
docker compose up -d
DATABASE_URL=postgres://idass:idass@localhost:5432/idass_eu_west_1 \
TEST_DATABASE_URL=postgres://idass:idass@localhost:5432/idass_eu_west_1 \
cargo test --workspace
```

### HTTP handler tests (no database needed)

```bash
cargo test -p api-gateway --test handler_tests
```

These use `mockall`-generated mocks for all port traits. Zero infrastructure required.

### OpenAPI contract tests (no database needed)

```bash
cargo test -p api-gateway --test contract_tests
```

Parses `openapi.yaml` and validates every response body against the declared schema. The `all_routes_are_documented_in_spec` test fails CI if a route is added without updating the spec.

### Geographic migration integration test (two databases)

```bash
docker compose -f docker-compose.test.yml up -d

DATABASE_URL=postgres://idass:idass@localhost:5432/idass_eu_west_1 \
TEST_DATABASE_URL=postgres://idass:idass@localhost:5434/idass_test_1 \
TEST_DATABASE_URL_2=postgres://idass:idass@localhost:5435/idass_test_2 \
cargo test -p api-gateway --test integration_test -- --include-ignored
```

### Full suite

```bash
docker compose up -d && docker compose -f docker-compose.test.yml up -d

DATABASE_URL=postgres://idass:idass@localhost:5432/idass_eu_west_1 \
TEST_DATABASE_URL=postgres://idass:idass@localhost:5432/idass_eu_west_1 \
TEST_DATABASE_URL_2=postgres://idass:idass@localhost:5433/idass_us_east_1 \
cargo test --workspace -- --include-ignored
```

**Test inventory (79 + 1 ignored):**

| Suite | Count | Infrastructure |
|---|---|---|
| `shared-kernel` unit tests | 8 | None |
| `mod-auth` integration tests | 7 | PostgreSQL + WireMock |
| `mod-token` integration tests | 6 | PostgreSQL + WireMock |
| `mod-mgmt` integration tests | 16 | PostgreSQL |
| `api-gateway` handler tests | 25 | None (mocks) |
| `api-gateway` contract tests | 15 | None (mocks) |
| `api-gateway` integration tests | 2 | PostgreSQL (2 instances) |
| `mod-mgmt` migration test | 1 `[ignore]` | PostgreSQL (2 instances) |

---

## Project Structure

```
idass/
├── rust-toolchain.toml       # Pins Rust 1.87.0; rustup activates automatically
├── .cargo/config.toml        # Project-scoped build settings
├── Cargo.toml                # Workspace root with shared dependency versions
├── openapi.yaml              # OpenAPI 3.1 spec — authoritative API contract
├── migrations/
│   └── 001_initial.sql       # Schema: tenants, orgs, connections, users, keys, outbox
├── docker-compose.yml        # postgres-eu, postgres-us, redis
├── docker-compose.test.yml   # Isolated test databases (ports 5434/5435)
├── shared-kernel/            # Domain models, AppError, AES-GCM-256 crypto
├── mod-auth/                 # Argon2id, Google OAuth2, SAML 2.0
├── mod-token/                # RS256 JWT, DashMap key cache, webhook enrichment
├── mod-mgmt/                 # Tenant/user/org CRUD, geographic migration
└── api-gateway/              # Axum server, port traits, router, OpenAPI contract tests
```

---

## Back Office Admin UI

A standalone React SPA (`idass-admin`) provides a visual interface for platform operators to manage tenants, connections, users, and organizations.

**Repository:** `idass-admin` (separate repo, deploys independently)

**Stack:** Vite + React 18 + TypeScript + shadcn/ui + Tailwind + TanStack Query + Axios

### Quick start (development)

```bash
# In the idass-admin repo
cp .env.example .env          # set VITE_API_URL and VITE_REGIONS
npm install
npm run dev                   # http://localhost:5173
```

Enter the `MANAGEMENT_API_KEY` from the backend `.env` on the login screen.

### Docker

```bash
docker build -t idass-admin .
docker run -p 3000:80 \
  -e VITE_API_URL=http://localhost:8080 \
  -e VITE_REGIONS=eu-west-1,us-east-1 \
  idass-admin
```

### Features

| Section | What you can do |
|---|---|
| Tenants | List, create, navigate to tenant workspace |
| Users | List all users per tenant, delete |
| Connections | List, create (database/OAuth2/SAML), edit with secret masking, delete |
| Organizations | List, create, delete |
| Migrate Region | 4-step wizard with tenant-name confirmation and progress polling |

The 503 "tenant locked" banner appears automatically when a tenant is migrating or in `read_only` state.

### E2E tests

```bash
# Requires: Docker stack running + MANAGEMENT_API_KEY in .env
cd idass-admin
npm run e2e          # run headless (19 tests, ~8 s)
npm run e2e:ui       # interactive Playwright UI
```

---

## Security Notes

- **Secrets at rest**: Connection options (OAuth2 `client_secret`, SAML certificates) are stored exclusively in `options_encrypted BYTEA` (AES-GCM-256). The `MASTER_ENCRYPTION_KEY` (32 bytes, base64-encoded) is the only secret that must be secured externally.
- **Multi-tenancy isolation**: Every SQL query in domain crates carries an explicit `WHERE tenant_id = $1` predicate. There is no shared query that crosses tenant boundaries.
- **Management API**: All `/api/v1/mgmt/` routes require `Authorization: Bearer <MANAGEMENT_API_KEY>`. The key is compared with `subtle::ConstantTimeEq` to prevent timing attacks.
- **CORS**: Only the origin in `ADMIN_ORIGIN` (default `http://localhost:5173`) is allowed. Methods restricted to `GET POST PUT DELETE`; headers to `Content-Type` and `Authorization`. Set `ADMIN_ORIGIN` to your production admin URL before deploying.
- **OAuth2 CSRF**: State stored in Redis with a 300-second TTL. Callback validates against both the `oauth_state` cookie (HttpOnly, Secure, SameSite=Lax) and the query parameter via constant-time comparison. Consumed atomically with `GETDEL`.
- **SAML**: `handle_acs_response` returns `400 InvalidConnection` until XML-DSIG signature validation is integrated. Do not enable SAML connections in production before completing this.

---

## Environment Variables

| Variable | Required | Description |
|---|---|---|
| `MASTER_ENCRYPTION_KEY` | Yes | Base64-encoded 32-byte AES key for encrypting connection secrets |
| `MANAGEMENT_API_KEY` | Yes | Bearer token for all `/api/v1/mgmt/` endpoints |
| `DATABASE_URL` | Yes (compile-time) | Default regional database; used by `sqlx` macro checks |
| `DATABASE_URL_EU_WEST_1` | Yes | PostgreSQL URL for the `eu-west-1` region |
| `DATABASE_URL_US_EAST_1` | No | PostgreSQL URL for the `us-east-1` region |
| `REGIONS` | No | Comma-separated region list (default: `eu-west-1`) |
| `REDIS_URL` | No | Redis URL (default: `redis://127.0.0.1/`) |
| `ISSUER` | No | JWT `iss` claim (default: `https://idass.example.com`) |
| `APP_PORT` | No | HTTP bind port (default: `8080`) |
| `ADMIN_ORIGIN` | No | Allowed CORS origin for the admin SPA (default: `http://localhost:5173`) |
| `RUST_LOG` | No | Log filter (recommended: `info,sqlx=warn`) |
