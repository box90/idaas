CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE TABLE IF NOT EXISTS tenants (
    id               UUID         PRIMARY KEY,
    name             VARCHAR(255) NOT NULL UNIQUE,
    current_region   VARCHAR(50)  NOT NULL,
    status           VARCHAR(50)  NOT NULL DEFAULT 'active',
    updated_at       TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS organizations (
    id           UUID         PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id    UUID         NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name         VARCHAR(255) NOT NULL,
    display_name VARCHAR(255),
    metadata     JSONB        DEFAULT '{}'::jsonb,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    UNIQUE(tenant_id, name)
);

CREATE TABLE IF NOT EXISTS connections (
    id                 UUID         PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id          UUID         NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name               VARCHAR(255) NOT NULL,
    strategy           VARCHAR(50)  NOT NULL,
    options_encrypted  BYTEA        NOT NULL,
    webhook_url        VARCHAR(512),
    created_at         TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    UNIQUE(tenant_id, name)
);

CREATE TABLE IF NOT EXISTS users (
    id                   UUID         PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id            UUID         NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    organization_id      UUID         REFERENCES organizations(id) ON DELETE SET NULL,
    connection_id        UUID         NOT NULL REFERENCES connections(id) ON DELETE RESTRICT,
    email                VARCHAR(255) NOT NULL,
    password_hash        VARCHAR(255),
    external_provider_id VARCHAR(255),
    user_metadata        JSONB        DEFAULT '{}'::jsonb,
    app_metadata         JSONB        DEFAULT '{}'::jsonb,
    created_at           TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at           TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    UNIQUE(tenant_id, connection_id, email)
);

CREATE TABLE IF NOT EXISTS tenant_keys (
    id                    UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id             UUID        NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    algorithm             VARCHAR(10) NOT NULL DEFAULT 'RS256',
    private_key_encrypted BYTEA       NOT NULL,
    public_key_pem        TEXT        NOT NULL,
    is_active             BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS migration_outbox (
    id         UUID         PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id  UUID         NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    event_type VARCHAR(100) NOT NULL,
    payload    JSONB        NOT NULL,
    processed  BOOLEAN      NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_users_tenant_email ON users(tenant_id, email);
CREATE INDEX IF NOT EXISTS idx_orgs_tenant        ON organizations(tenant_id);
CREATE INDEX IF NOT EXISTS idx_tenant_keys_active ON tenant_keys(tenant_id, is_active);
