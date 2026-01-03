//! Database Migrations - Schema Management
//!
//! Embedded SQL migrations that run automatically on startup.
//! Supports both PostgreSQL and SQLite with automatic dialect detection.

use super::{DatabaseError, UnifiedPool, DatabaseBackend};

#[cfg(feature = "postgres")]
use sqlx::postgres::PgPool;

#[cfg(feature = "sqlite")]
use sqlx::sqlite::SqlitePool;

/// Run all migrations for the given pool
pub async fn run_migrations(pool: &UnifiedPool) -> Result<(), DatabaseError> {
    match pool.backend_type() {
        #[cfg(feature = "postgres")]
        Some(DatabaseBackend::PostgreSQL) => {
            let pg = pool.as_postgres()?;
            run_postgres_migrations(pg).await
        }
        #[cfg(feature = "sqlite")]
        Some(DatabaseBackend::SQLite | DatabaseBackend::SQLiteMemory) => {
            let sqlite = pool.as_sqlite()?;
            run_sqlite_migrations(sqlite).await
        }
        _ => Err(DatabaseError::NotConnected),
    }
}

/// Run PostgreSQL-specific migrations
#[cfg(feature = "postgres")]
async fn run_postgres_migrations(pool: &PgPool) -> Result<(), DatabaseError> {
    log::info!("Running PostgreSQL migrations...");

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
    let migrations = get_postgres_migrations();
    
    for (name, sql) in migrations {
        let applied: Option<(i32,)> = sqlx::query_as(
            "SELECT id FROM _migrations WHERE name = $1"
        )
        .bind(name)
        .fetch_optional(pool)
        .await
        .map_err(|e| DatabaseError::MigrationFailed(e.to_string()))?;

        if applied.is_none() {
            log::info!("Applying PostgreSQL migration: {}", name);
            
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

    log::info!("PostgreSQL migrations complete");
    Ok(())
}

/// Run SQLite-specific migrations
#[cfg(feature = "sqlite")]
async fn run_sqlite_migrations(pool: &SqlitePool) -> Result<(), DatabaseError> {
    log::info!("Running SQLite migrations...");

    // Create migrations tracking table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS _migrations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            applied_at TEXT DEFAULT (datetime('now'))
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| DatabaseError::MigrationFailed(e.to_string()))?;

    // Run each migration if not already applied
    let migrations = get_sqlite_migrations();
    
    for (name, sql) in migrations {
        let applied: Option<(i32,)> = sqlx::query_as(
            "SELECT id FROM _migrations WHERE name = ?"
        )
        .bind(name)
        .fetch_optional(pool)
        .await
        .map_err(|e| DatabaseError::MigrationFailed(e.to_string()))?;

        if applied.is_none() {
            log::info!("Applying SQLite migration: {}", name);
            
            // SQLite doesn't support multiple statements well, so split them
            for statement in sql.split(';').filter(|s| !s.trim().is_empty()) {
                sqlx::query(statement.trim())
                    .execute(pool)
                    .await
                    .map_err(|e| DatabaseError::MigrationFailed(format!("{}: {}", name, e)))?;
            }

            sqlx::query("INSERT INTO _migrations (name) VALUES (?)")
                .bind(name)
                .execute(pool)
                .await
                .map_err(|e| DatabaseError::MigrationFailed(e.to_string()))?;
        }
    }

    log::info!("SQLite migrations complete");
    Ok(())
}

// ============================================================================
// PostgreSQL Migrations
// ============================================================================

#[cfg(feature = "postgres")]
fn get_postgres_migrations() -> Vec<(&'static str, &'static str)> {
    vec![
        ("001_create_enums", PG_MIGRATION_001_ENUMS),
        ("002_create_users", PG_MIGRATION_002_USERS),
        ("003_create_sessions", PG_MIGRATION_003_SESSIONS),
        ("004_create_audit_logs", PG_MIGRATION_004_AUDIT_LOGS),
        ("005_create_settings", PG_MIGRATION_005_SETTINGS),
        ("006_create_vms", PG_MIGRATION_006_VMS),
        ("007_create_api_keys", PG_MIGRATION_007_API_KEYS),
        ("008_create_indexes", PG_MIGRATION_008_INDEXES),
    ]
}

// ============================================================================
// SQLite Migrations
// ============================================================================

#[cfg(feature = "sqlite")]
fn get_sqlite_migrations() -> Vec<(&'static str, &'static str)> {
    vec![
        ("001_create_users", SQLITE_MIGRATION_001_USERS),
        ("002_create_sessions", SQLITE_MIGRATION_002_SESSIONS),
        ("003_create_audit_logs", SQLITE_MIGRATION_003_AUDIT_LOGS),
        ("004_create_settings", SQLITE_MIGRATION_004_SETTINGS),
        ("005_create_vms", SQLITE_MIGRATION_005_VMS),
        ("006_create_api_keys", SQLITE_MIGRATION_006_API_KEYS),
        ("007_create_indexes", SQLITE_MIGRATION_007_INDEXES),
    ]
}

// ============================================================================
// PostgreSQL Migration SQL
// ============================================================================

#[cfg(feature = "postgres")]
const PG_MIGRATION_001_ENUMS: &str = r#"
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

#[cfg(feature = "postgres")]
const PG_MIGRATION_002_USERS: &str = r#"
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

COMMENT ON TABLE users IS 'User accounts for NVM platform';
COMMENT ON COLUMN users.password_hash IS 'Argon2id hashed password';
"#;

#[cfg(feature = "postgres")]
const PG_MIGRATION_003_SESSIONS: &str = r#"
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
"#;

#[cfg(feature = "postgres")]
const PG_MIGRATION_004_AUDIT_LOGS: &str = r#"
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
"#;

#[cfg(feature = "postgres")]
const PG_MIGRATION_005_SETTINGS: &str = r#"
CREATE TABLE IF NOT EXISTS settings (
    key VARCHAR(128) PRIMARY KEY,
    value JSONB NOT NULL,
    description TEXT,
    category VARCHAR(32) NOT NULL DEFAULT 'general',
    is_secret BOOLEAN NOT NULL DEFAULT false,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_by UUID REFERENCES users(id) ON DELETE SET NULL
);

INSERT INTO settings (key, value, description, category) VALUES
    ('system.name', '"NVM Enterprise"', 'System display name', 'general'),
    ('system.timezone', '"UTC"', 'Default timezone', 'general'),
    ('auth.session_timeout', '28800', 'Session timeout in seconds', 'auth'),
    ('vm.default_memory_mb', '2048', 'Default VM memory', 'vm'),
    ('vm.default_vcpus', '2', 'Default VM vCPUs', 'vm')
ON CONFLICT (key) DO NOTHING;
"#;

#[cfg(feature = "postgres")]
const PG_MIGRATION_006_VMS: &str = r#"
CREATE TABLE IF NOT EXISTS vms (
    id UUID PRIMARY KEY,
    name VARCHAR(64) NOT NULL UNIQUE,
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
    config JSONB DEFAULT '{}'
);
"#;

#[cfg(feature = "postgres")]
const PG_MIGRATION_007_API_KEYS: &str = r#"
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
"#;

#[cfg(feature = "postgres")]
const PG_MIGRATION_008_INDEXES: &str = r#"
CREATE INDEX IF NOT EXISTS idx_users_role ON users(role);
CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_logs(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_vms_status ON vms(status);
CREATE INDEX IF NOT EXISTS idx_api_keys_user ON api_keys(user_id);
"#;

// ============================================================================
// SQLite Migration SQL
// ============================================================================

#[cfg(feature = "sqlite")]
const SQLITE_MIGRATION_001_USERS: &str = r#"
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    display_name TEXT,
    role TEXT NOT NULL DEFAULT 'viewer' CHECK(role IN ('admin', 'operator', 'viewer', 'auditor')),
    is_active INTEGER NOT NULL DEFAULT 1,
    is_locked INTEGER NOT NULL DEFAULT 0,
    failed_login_attempts INTEGER NOT NULL DEFAULT 0,
    last_login TEXT,
    last_password_change TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL
)
"#;

#[cfg(feature = "sqlite")]
const SQLITE_MIGRATION_002_SESSIONS: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL,
    csrf_token TEXT NOT NULL,
    ip_address TEXT,
    user_agent TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL,
    last_activity TEXT NOT NULL DEFAULT (datetime('now')),
    is_revoked INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_token ON sessions(token_hash)
"#;

#[cfg(feature = "sqlite")]
const SQLITE_MIGRATION_003_AUDIT_LOGS: &str = r#"
CREATE TABLE IF NOT EXISTS audit_logs (
    id TEXT PRIMARY KEY,
    timestamp TEXT NOT NULL DEFAULT (datetime('now')),
    user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    username TEXT,
    action TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT,
    resource_name TEXT,
    details TEXT,
    ip_address TEXT,
    user_agent TEXT,
    status TEXT NOT NULL DEFAULT 'success' CHECK(status IN ('success', 'failure', 'denied')),
    error_message TEXT
)
"#;

#[cfg(feature = "sqlite")]
const SQLITE_MIGRATION_004_SETTINGS: &str = r#"
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    description TEXT,
    category TEXT NOT NULL DEFAULT 'general',
    is_secret INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_by TEXT REFERENCES users(id) ON DELETE SET NULL
);
INSERT OR IGNORE INTO settings (key, value, description, category) VALUES
    ('system.name', '"NVM Enterprise"', 'System display name', 'general');
INSERT OR IGNORE INTO settings (key, value, description, category) VALUES
    ('system.timezone', '"UTC"', 'Default timezone', 'general');
INSERT OR IGNORE INTO settings (key, value, description, category) VALUES
    ('auth.session_timeout', '28800', 'Session timeout in seconds', 'auth');
INSERT OR IGNORE INTO settings (key, value, description, category) VALUES
    ('vm.default_memory_mb', '2048', 'Default VM memory', 'vm');
INSERT OR IGNORE INTO settings (key, value, description, category) VALUES
    ('vm.default_vcpus', '2', 'Default VM vCPUs', 'vm')
"#;

#[cfg(feature = "sqlite")]
const SQLITE_MIGRATION_005_VMS: &str = r#"
CREATE TABLE IF NOT EXISTS vms (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    vcpus INTEGER NOT NULL DEFAULT 2,
    memory_mb INTEGER NOT NULL DEFAULT 2048,
    disk_gb INTEGER NOT NULL DEFAULT 20,
    status TEXT NOT NULL DEFAULT 'stopped',
    os_type TEXT,
    template_id TEXT,
    node_id TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    tags TEXT DEFAULT '[]',
    config TEXT DEFAULT '{}'
)
"#;

#[cfg(feature = "sqlite")]
const SQLITE_MIGRATION_006_API_KEYS: &str = r#"
CREATE TABLE IF NOT EXISTS api_keys (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    key_hash TEXT NOT NULL,
    prefix TEXT NOT NULL,
    permissions TEXT NOT NULL DEFAULT '[]',
    expires_at TEXT,
    last_used TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    is_revoked INTEGER NOT NULL DEFAULT 0,
    
    UNIQUE (user_id, name)
)
"#;

#[cfg(feature = "sqlite")]
const SQLITE_MIGRATION_007_INDEXES: &str = r#"
CREATE INDEX IF NOT EXISTS idx_users_role ON users(role);
CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_logs(timestamp);
CREATE INDEX IF NOT EXISTS idx_vms_status ON vms(status);
CREATE INDEX IF NOT EXISTS idx_api_keys_user ON api_keys(user_id)
"#;
