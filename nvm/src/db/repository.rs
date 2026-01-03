//! Database Repositories - Data Access Layer
//!
//! Provides async repository pattern for database operations.
//! Supports both PostgreSQL and SQLite backends through unified traits.

use super::models::*;
use super::DatabaseError;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[cfg(feature = "postgres")]
use sqlx::PgPool;

#[cfg(feature = "sqlite")]
use sqlx::SqlitePool;

// ============================================================================
// Password Hashing Helpers
// ============================================================================

fn hash_password(password: &str) -> Result<String, DatabaseError> {
    use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
    use rand::rngs::OsRng;

    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| DatabaseError::InvalidData(e.to_string()))
        .map(|h| h.to_string())
}

fn verify_password(password: &str, hash: &str) -> bool {
    use argon2::{Argon2, PasswordHash, PasswordVerifier};

    let parsed_hash = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

// ============================================================================
// User Repository Trait
// ============================================================================

/// Trait for user repository operations (database-agnostic)
#[allow(async_fn_in_trait)]
pub trait UserRepositoryTrait: Send + Sync {
    async fn create(&self, user: &CreateUser) -> Result<User, DatabaseError>;
    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, DatabaseError>;
    async fn find_by_username(&self, username: &str) -> Result<Option<User>, DatabaseError>;
    async fn verify_credentials(&self, username: &str, password: &str) -> Result<Option<User>, DatabaseError>;
    async fn list(&self, offset: i64, limit: i64) -> Result<(Vec<User>, i64), DatabaseError>;
    async fn update(&self, id: Uuid, update: &UpdateUser) -> Result<User, DatabaseError>;
    async fn delete(&self, id: Uuid) -> Result<(), DatabaseError>;
    async fn change_password(&self, id: Uuid, new_password: &str) -> Result<(), DatabaseError>;
}

// ============================================================================
// PostgreSQL User Repository
// ============================================================================

#[cfg(feature = "postgres")]
pub struct UserRepository {
    pool: PgPool,
}

#[cfg(feature = "postgres")]
impl UserRepository {
    pub fn new_pg(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[cfg(feature = "postgres")]
impl UserRepositoryTrait for UserRepository {
    async fn create(&self, user: &CreateUser) -> Result<User, DatabaseError> {
        let password_hash = hash_password(&user.password)?;
        let id = Uuid::new_v4();
        let now = Utc::now();

        let result = sqlx::query_as::<_, User>(
            r#"
            INSERT INTO users (id, username, email, password_hash, display_name, role, 
                              is_active, is_locked, failed_login_attempts, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, true, false, 0, $7, $7)
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(&user.username)
        .bind(&user.email)
        .bind(&password_hash)
        .bind(&user.display_name)
        .bind(&user.role)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;

        Ok(result)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, DatabaseError> {
        let result = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(result)
    }

    async fn find_by_username(&self, username: &str) -> Result<Option<User>, DatabaseError> {
        let result = sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = $1")
            .bind(username)
            .fetch_optional(&self.pool)
            .await?;
        Ok(result)
    }

    async fn verify_credentials(&self, username: &str, password: &str) -> Result<Option<User>, DatabaseError> {
        let user = match self.find_by_username(username).await? {
            Some(u) => u,
            None => return Ok(None),
        };

        if user.is_locked || !user.is_active {
            return Ok(None);
        }

        if verify_password(password, &user.password_hash) {
            sqlx::query("UPDATE users SET last_login = $1, failed_login_attempts = 0 WHERE id = $2")
                .bind(Utc::now())
                .bind(user.id)
                .execute(&self.pool)
                .await?;
            Ok(Some(user))
        } else {
            sqlx::query(
                "UPDATE users SET failed_login_attempts = failed_login_attempts + 1, 
                 is_locked = (failed_login_attempts >= 5) WHERE id = $1"
            )
            .bind(user.id)
            .execute(&self.pool)
            .await?;
            Ok(None)
        }
    }

    async fn list(&self, offset: i64, limit: i64) -> Result<(Vec<User>, i64), DatabaseError> {
        let users = sqlx::query_as::<_, User>(
            "SELECT * FROM users ORDER BY created_at DESC OFFSET $1 LIMIT $2"
        )
        .bind(offset)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await?;

        Ok((users, count.0))
    }

    async fn update(&self, id: Uuid, update: &UpdateUser) -> Result<User, DatabaseError> {
        let user = self.find_by_id(id).await?.ok_or(DatabaseError::NotFound)?;

        let email = update.email.as_ref().unwrap_or(&user.email);
        let display_name = update.display_name.as_ref().or(user.display_name.as_ref());
        let role = update.role.unwrap_or(user.role);
        let is_active = update.is_active.unwrap_or(user.is_active);

        let result = sqlx::query_as::<_, User>(
            r#"
            UPDATE users 
            SET email = $1, display_name = $2, role = $3, is_active = $4, updated_at = $5
            WHERE id = $6
            RETURNING *
            "#,
        )
        .bind(email)
        .bind(display_name)
        .bind(role)
        .bind(is_active)
        .bind(Utc::now())
        .bind(id)
        .fetch_one(&self.pool)
        .await?;

        Ok(result)
    }

    async fn delete(&self, id: Uuid) -> Result<(), DatabaseError> {
        let result = sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(DatabaseError::NotFound);
        }
        Ok(())
    }

    async fn change_password(&self, id: Uuid, new_password: &str) -> Result<(), DatabaseError> {
        let password_hash = hash_password(new_password)?;
        sqlx::query(
            "UPDATE users SET password_hash = $1, last_password_change = $2, updated_at = $2 WHERE id = $3"
        )
        .bind(&password_hash)
        .bind(Utc::now())
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

// ============================================================================
// SQLite User Repository
// ============================================================================

#[cfg(feature = "sqlite")]
pub struct SqliteUserRepository {
    pool: SqlitePool,
}

#[cfg(feature = "sqlite")]
impl SqliteUserRepository {
    pub fn new_sqlite(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[cfg(feature = "sqlite")]
impl UserRepositoryTrait for SqliteUserRepository {
    async fn create(&self, user: &CreateUser) -> Result<User, DatabaseError> {
        let password_hash = hash_password(&user.password)?;
        let id = Uuid::new_v4();
        let now = Utc::now();

        // SQLite doesn't support RETURNING *, so we insert then select
        sqlx::query(
            r#"
            INSERT INTO users (id, username, email, password_hash, display_name, role, 
                              is_active, is_locked, failed_login_attempts, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, 1, 0, 0, ?, ?)
            "#,
        )
        .bind(id.to_string())
        .bind(&user.username)
        .bind(&user.email)
        .bind(&password_hash)
        .bind(&user.display_name)
        .bind(user.role.to_string())
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        self.find_by_id(id).await?.ok_or(DatabaseError::NotFound)
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, DatabaseError> {
        let row = sqlx::query_as::<_, SqliteUserRow>("SELECT * FROM users WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.into()))
    }

    async fn find_by_username(&self, username: &str) -> Result<Option<User>, DatabaseError> {
        let row = sqlx::query_as::<_, SqliteUserRow>("SELECT * FROM users WHERE username = ?")
            .bind(username)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.into()))
    }

    async fn verify_credentials(&self, username: &str, password: &str) -> Result<Option<User>, DatabaseError> {
        let user = match self.find_by_username(username).await? {
            Some(u) => u,
            None => return Ok(None),
        };

        if user.is_locked || !user.is_active {
            return Ok(None);
        }

        if verify_password(password, &user.password_hash) {
            sqlx::query("UPDATE users SET last_login = ?, failed_login_attempts = 0 WHERE id = ?")
                .bind(Utc::now().to_rfc3339())
                .bind(user.id.to_string())
                .execute(&self.pool)
                .await?;
            Ok(Some(user))
        } else {
            sqlx::query(
                "UPDATE users SET failed_login_attempts = failed_login_attempts + 1, 
                 is_locked = CASE WHEN failed_login_attempts >= 5 THEN 1 ELSE 0 END WHERE id = ?"
            )
            .bind(user.id.to_string())
            .execute(&self.pool)
            .await?;
            Ok(None)
        }
    }

    async fn list(&self, offset: i64, limit: i64) -> Result<(Vec<User>, i64), DatabaseError> {
        let rows = sqlx::query_as::<_, SqliteUserRow>(
            "SELECT * FROM users ORDER BY created_at DESC LIMIT ? OFFSET ?"
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await?;

        Ok((rows.into_iter().map(|r| r.into()).collect(), count.0))
    }

    async fn update(&self, id: Uuid, update: &UpdateUser) -> Result<User, DatabaseError> {
        let user = self.find_by_id(id).await?.ok_or(DatabaseError::NotFound)?;

        let email = update.email.as_ref().unwrap_or(&user.email);
        let display_name = update.display_name.as_ref().or(user.display_name.as_ref());
        let role = update.role.unwrap_or(user.role);
        let is_active = update.is_active.unwrap_or(user.is_active);

        sqlx::query(
            "UPDATE users SET email = ?, display_name = ?, role = ?, is_active = ?, updated_at = ? WHERE id = ?"
        )
        .bind(email)
        .bind(display_name)
        .bind(role.to_string())
        .bind(if is_active { 1i32 } else { 0i32 })
        .bind(Utc::now().to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;

        self.find_by_id(id).await?.ok_or(DatabaseError::NotFound)
    }

    async fn delete(&self, id: Uuid) -> Result<(), DatabaseError> {
        let result = sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(DatabaseError::NotFound);
        }
        Ok(())
    }

    async fn change_password(&self, id: Uuid, new_password: &str) -> Result<(), DatabaseError> {
        let password_hash = hash_password(new_password)?;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE users SET password_hash = ?, last_password_change = ?, updated_at = ? WHERE id = ?"
        )
        .bind(&password_hash)
        .bind(&now)
        .bind(&now)
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

// SQLite row type for User (handles type conversions)
#[cfg(feature = "sqlite")]
#[derive(sqlx::FromRow)]
struct SqliteUserRow {
    id: String,
    username: String,
    email: String,
    password_hash: String,
    display_name: Option<String>,
    role: String,
    is_active: i32,
    is_locked: i32,
    failed_login_attempts: i32,
    last_login: Option<String>,
    last_password_change: Option<String>,
    created_at: String,
    updated_at: String,
    created_by: Option<String>,
}

#[cfg(feature = "sqlite")]
impl From<SqliteUserRow> for User {
    fn from(row: SqliteUserRow) -> Self {
        use std::str::FromStr;
        User {
            id: Uuid::parse_str(&row.id).unwrap_or_default(),
            username: row.username,
            email: row.email,
            password_hash: row.password_hash,
            display_name: row.display_name,
            role: match row.role.as_str() {
                "admin" => UserRole::Admin,
                "operator" => UserRole::Operator,
                "auditor" => UserRole::Auditor,
                _ => UserRole::Viewer,
            },
            is_active: row.is_active != 0,
            is_locked: row.is_locked != 0,
            failed_login_attempts: row.failed_login_attempts,
            last_login: row.last_login.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))),
            last_password_change: row.last_password_change.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|d| d.with_timezone(&Utc))),
            created_at: DateTime::parse_from_rfc3339(&row.created_at).map(|d| d.with_timezone(&Utc)).unwrap_or_else(|_| Utc::now()),
            updated_at: DateTime::parse_from_rfc3339(&row.updated_at).map(|d| d.with_timezone(&Utc)).unwrap_or_else(|_| Utc::now()),
            created_by: row.created_by.and_then(|s| Uuid::parse_str(&s).ok()),
        }
    }
}

// ============================================================================
// Unified Repository Constructor
// ============================================================================

#[cfg(feature = "postgres")]
impl UserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self::new_pg(pool)
    }
}

#[cfg(feature = "sqlite")]
impl SqliteUserRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self::new_sqlite(pool)
    }
}

// ============================================================================
// Session Repository (simplified - PostgreSQL only for now)
// ============================================================================

#[cfg(feature = "postgres")]
pub struct SessionRepository {
    pool: PgPool,
}

#[cfg(feature = "postgres")]
impl SessionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, session: &CreateSession) -> Result<DbSession, DatabaseError> {
        use sha2::{Digest, Sha256};

        let id = Uuid::new_v4();
        let now = Utc::now();

        let mut hasher = Sha256::new();
        hasher.update(session.token.as_bytes());
        let token_hash = hex::encode(hasher.finalize());

        let result = sqlx::query_as::<_, DbSession>(
            r#"
            INSERT INTO sessions (id, user_id, token_hash, csrf_token, ip_address, 
                                 user_agent, created_at, expires_at, last_activity, is_revoked)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $7, false)
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(session.user_id)
        .bind(&token_hash)
        .bind(&session.csrf_token)
        .bind(&session.ip_address)
        .bind(&session.user_agent)
        .bind(now)
        .bind(session.expires_at)
        .fetch_one(&self.pool)
        .await?;

        Ok(result)
    }

    pub async fn find_by_token(&self, token: &str) -> Result<Option<DbSession>, DatabaseError> {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        let token_hash = hex::encode(hasher.finalize());

        let result = sqlx::query_as::<_, DbSession>(
            "SELECT * FROM sessions WHERE token_hash = $1 AND is_revoked = false AND expires_at > $2"
        )
        .bind(&token_hash)
        .bind(Utc::now())
        .fetch_optional(&self.pool)
        .await?;

        Ok(result)
    }

    pub async fn revoke(&self, id: Uuid) -> Result<(), DatabaseError> {
        sqlx::query("UPDATE sessions SET is_revoked = true WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn cleanup_expired(&self) -> Result<u64, DatabaseError> {
        let result = sqlx::query("DELETE FROM sessions WHERE expires_at < $1 OR is_revoked = true")
            .bind(Utc::now())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}

// ============================================================================
// Settings Repository
// ============================================================================

#[cfg(feature = "postgres")]
pub struct SettingsRepository {
    pool: PgPool,
}

#[cfg(feature = "postgres")]
impl SettingsRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn get(&self, key: &str) -> Result<Option<Setting>, DatabaseError> {
        let result = sqlx::query_as::<_, Setting>("SELECT * FROM settings WHERE key = $1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(result)
    }

    pub async fn set(
        &self,
        key: &str,
        value: serde_json::Value,
        user_id: Option<Uuid>,
    ) -> Result<Setting, DatabaseError> {
        let result = sqlx::query_as::<_, Setting>(
            r#"
            INSERT INTO settings (key, value, updated_at, updated_by)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (key) DO UPDATE
            SET value = $2, updated_at = $3, updated_by = $4
            RETURNING *
            "#,
        )
        .bind(key)
        .bind(&value)
        .bind(Utc::now())
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(result)
    }

    pub async fn get_by_category(&self, category: &str) -> Result<Vec<Setting>, DatabaseError> {
        let results = sqlx::query_as::<_, Setting>(
            "SELECT * FROM settings WHERE category = $1 ORDER BY key"
        )
        .bind(category)
        .fetch_all(&self.pool)
        .await?;
        Ok(results)
    }
}

// ============================================================================
// Audit Log Repository  
// ============================================================================

#[cfg(feature = "postgres")]
pub struct AuditLogRepository {
    pool: PgPool,
}

#[cfg(feature = "postgres")]
impl AuditLogRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, entry: &CreateAuditLog) -> Result<AuditLog, DatabaseError> {
        let id = Uuid::new_v4();
        let now = Utc::now();

        let result = sqlx::query_as::<_, AuditLog>(
            r#"
            INSERT INTO audit_logs (id, timestamp, user_id, username, action, resource_type,
                                   resource_id, resource_name, details, ip_address, 
                                   user_agent, status, error_message)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(now)
        .bind(entry.user_id)
        .bind(&entry.username)
        .bind(&entry.action)
        .bind(&entry.resource_type)
        .bind(&entry.resource_id)
        .bind(&entry.resource_name)
        .bind(&entry.details)
        .bind(&entry.ip_address)
        .bind(&entry.user_agent)
        .bind(entry.status)
        .bind(&entry.error_message)
        .fetch_one(&self.pool)
        .await?;

        Ok(result)
    }
}
