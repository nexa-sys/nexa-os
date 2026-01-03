//! Database Migrations - Schema Management
//!
//! Embedded SQL migrations that run automatically on startup.

use super::DatabaseError;

#[cfg(feature = "database")]
use sqlx::PgPool;

/// Run all migrations
#[cfg(feature = "database")]
pub async fn run_migrations(pool: &PgPool) -> Result<(), DatabaseError> {
    log::info!("Running database migrations...");

    // Create migrations tracking table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS _migrations (
            id SERIAL PRIMARY KEY,
            name VARCHAR(255) NOT NULL UNIQUE,
            applied_at TIMESTAMPTZ DEFAULT NOW()
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| DatabaseError::MigrationFailed(e.to_string()))?;

    // Run each migration if not already applied
    let migrations = get_migrations();
    
    for (name, sql) in migrations {
        let applied: Option<(i32,)> = sqlx::query_as(
            "SELECT id FROM _migrations WHERE name = $1"
        )
        .bind(name)
        .fetch_optional(pool)
        .await
        .map_err(|e| DatabaseError::MigrationFailed(e.to_string()))?;

        if applied.is_none() {
            log::info!("Applying migration: {}", name);
            
            sqlx::query(sql)
                .execute(pool)
                .await
                .map_err(|e| DatabaseError::MigrationFailed(format!("{}: {}", name, e)))?;

            sqlx::query("INSERT INTO _migrations (name) VALUES ($1)")
                .bind(name)
                .execute(pool)
                .await
                .map_err(|e| DatabaseError::MigrationFailed(e.to_string()))?;
        }
    }

    log::info!("Database migrations complete");
    Ok(())
}

/// Get all migrations in order
fn get_migrations() -> Vec<(&'static str, &'static str)> {
    vec![
        ("001_create_enums", MIGRATION_001_ENUMS),
        ("002_create_users", MIGRATION_002_USERS),
        ("003_create_sessions", MIGRATION_003_SESSIONS),
        ("004_create_audit_logs", MIGRATION_004_AUDIT_LOGS),
        ("005_create_settings", MIGRATION_005_SETTINGS),
        ("006_create_vms", MIGRATION_006_VMS),
        ("007_create_api_keys", MIGRATION_007_API_KEYS),
        ("008_create_indexes", MIGRATION_008_INDEXES),
    ]
}

// ============================================================================
// Migration SQL
// ============================================================================

const MIGRATION_001_ENUMS: &str = r#"
-- User roles enum
DO $$ BEGIN
    CREATE TYPE user_role AS ENUM ('admin', 'operator', 'viewer', 'auditor');
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

-- Audit status enum
DO $$ BEGIN
    CREATE TYPE audit_status AS ENUM ('success', 'failure', 'denied');
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;
"#;

const MIGRATION_002_USERS: &str = r#"
CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY,
    username VARCHAR(64) NOT NULL UNIQUE,
    email VARCHAR(255) NOT NULL UNIQUE,
    password_hash VARCHAR(255) NOT NULL,
    display_name VARCHAR(128),
    role user_role NOT NULL DEFAULT 'viewer',
    is_active BOOLEAN NOT NULL DEFAULT true,
    is_locked BOOLEAN NOT NULL DEFAULT false,
    failed_login_attempts INTEGER NOT NULL DEFAULT 0,
    last_login TIMESTAMPTZ,
    last_password_change TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    
    CONSTRAINT username_length CHECK (LENGTH(username) >= 3),
    CONSTRAINT email_format CHECK (email ~* '^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$')
);

-- Add comments
COMMENT ON TABLE users IS 'User accounts for NVM platform';
COMMENT ON COLUMN users.password_hash IS 'Argon2id hashed password';
COMMENT ON COLUMN users.failed_login_attempts IS 'Counter for account lockout (locks at 5)';
"#;

const MIGRATION_003_SESSIONS: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash VARCHAR(64) NOT NULL,
    csrf_token VARCHAR(64) NOT NULL,
    ip_address VARCHAR(45),
    user_agent TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    last_activity TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    is_revoked BOOLEAN NOT NULL DEFAULT false
);

CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_token ON sessions(token_hash);
CREATE INDEX IF NOT EXISTS idx_sessions_expires ON sessions(expires_at) WHERE is_revoked = false;

COMMENT ON TABLE sessions IS 'User authentication sessions';
COMMENT ON COLUMN sessions.token_hash IS 'SHA-256 hash of session token';
"#;

const MIGRATION_004_AUDIT_LOGS: &str = r#"
CREATE TABLE IF NOT EXISTS audit_logs (
    id UUID PRIMARY KEY,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    username VARCHAR(64),
    action VARCHAR(64) NOT NULL,
    resource_type VARCHAR(32) NOT NULL,
    resource_id VARCHAR(64),
    resource_name VARCHAR(255),
    details JSONB,
    ip_address VARCHAR(45),
    user_agent TEXT,
    status audit_status NOT NULL DEFAULT 'success',
    error_message TEXT
);

-- Partition by month for large deployments (optional)
-- CREATE TABLE audit_logs_2024_01 PARTITION OF audit_logs
--     FOR VALUES FROM ('2024-01-01') TO ('2024-02-01');

COMMENT ON TABLE audit_logs IS 'Security and operational audit trail';
"#;

const MIGRATION_005_SETTINGS: &str = r#"
CREATE TABLE IF NOT EXISTS settings (
    key VARCHAR(128) PRIMARY KEY,
    value JSONB NOT NULL,
    description TEXT,
    category VARCHAR(32) NOT NULL DEFAULT 'general',
    is_secret BOOLEAN NOT NULL DEFAULT false,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_by UUID REFERENCES users(id) ON DELETE SET NULL
);

-- Insert default settings
INSERT INTO settings (key, value, description, category) VALUES
    ('system.name', '"NVM Enterprise"', 'System display name', 'general'),
    ('system.timezone', '"UTC"', 'Default timezone', 'general'),
    ('auth.session_timeout', '28800', 'Session timeout in seconds (8 hours)', 'auth'),
    ('auth.max_sessions_per_user', '10', 'Maximum concurrent sessions', 'auth'),
    ('auth.require_2fa', 'false', 'Require two-factor authentication', 'auth'),
    ('auth.password_min_length', '8', 'Minimum password length', 'auth'),
    ('backup.retention_days', '30', 'Backup retention period', 'backup'),
    ('cluster.heartbeat_interval', '5', 'Cluster heartbeat interval (seconds)', 'cluster'),
    ('vm.default_memory_mb', '2048', 'Default VM memory', 'vm'),
    ('vm.default_vcpus', '2', 'Default VM vCPUs', 'vm')
ON CONFLICT (key) DO NOTHING;

COMMENT ON TABLE settings IS 'System configuration settings';
"#;

const MIGRATION_006_VMS: &str = r#"
CREATE TABLE IF NOT EXISTS vms (
    id UUID PRIMARY KEY,
    name VARCHAR(64) NOT NULL,
    description TEXT,
    vcpus INTEGER NOT NULL DEFAULT 2,
    memory_mb BIGINT NOT NULL DEFAULT 2048,
    disk_gb BIGINT NOT NULL DEFAULT 20,
    status VARCHAR(32) NOT NULL DEFAULT 'stopped',
    os_type VARCHAR(32),
    template_id UUID,
    node_id VARCHAR(64),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    tags JSONB DEFAULT '[]',
    config JSONB DEFAULT '{}',
    
    CONSTRAINT vm_name_unique UNIQUE (name),
    CONSTRAINT vcpus_positive CHECK (vcpus > 0),
    CONSTRAINT memory_positive CHECK (memory_mb > 0)
);

CREATE INDEX IF NOT EXISTS idx_vms_status ON vms(status);
CREATE INDEX IF NOT EXISTS idx_vms_node ON vms(node_id);
CREATE INDEX IF NOT EXISTS idx_vms_created_by ON vms(created_by);

COMMENT ON TABLE vms IS 'Virtual machine definitions';
"#;

const MIGRATION_007_API_KEYS: &str = r#"
CREATE TABLE IF NOT EXISTS api_keys (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name VARCHAR(64) NOT NULL,
    key_hash VARCHAR(64) NOT NULL,
    prefix VARCHAR(8) NOT NULL,
    permissions JSONB NOT NULL DEFAULT '[]',
    expires_at TIMESTAMPTZ,
    last_used TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    is_revoked BOOLEAN NOT NULL DEFAULT false,
    
    CONSTRAINT api_key_name_user_unique UNIQUE (user_id, name)
);

CREATE INDEX IF NOT EXISTS idx_api_keys_user ON api_keys(user_id);
CREATE INDEX IF NOT EXISTS idx_api_keys_prefix ON api_keys(prefix);

COMMENT ON TABLE api_keys IS 'API keys for programmatic access';
COMMENT ON COLUMN api_keys.prefix IS 'First 8 characters for key identification';
"#;

const MIGRATION_008_INDEXES: &str = r#"
-- Performance indexes for common queries
CREATE INDEX IF NOT EXISTS idx_users_role ON users(role);
CREATE INDEX IF NOT EXISTS idx_users_active ON users(is_active) WHERE is_active = true;
CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_logs(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_audit_user ON audit_logs(user_id);
CREATE INDEX IF NOT EXISTS idx_audit_action ON audit_logs(action);
CREATE INDEX IF NOT EXISTS idx_audit_resource ON audit_logs(resource_type, resource_id);

-- Full-text search on VM names (optional)
-- CREATE INDEX IF NOT EXISTS idx_vms_name_search ON vms USING gin(to_tsvector('english', name));

-- Analyze tables for query optimization
ANALYZE users;
ANALYZE sessions;
ANALYZE audit_logs;
ANALYZE settings;
ANALYZE vms;
ANALYZE api_keys;
"#;
