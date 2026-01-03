//! Database Repositories - Data Access Layer
//!
//! Provides async repository pattern for database operations.

use super::models::*;
use super::DatabaseError;
use chrono::{Duration, Utc};
use uuid::Uuid;

#[cfg(feature = "database")]
use sqlx::PgPool;

// ============================================================================
// User Repository
// ============================================================================

pub struct UserRepository {
    #[cfg(feature = "database")]
    pool: PgPool,
}

impl UserRepository {
    #[cfg(feature = "database")]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Create a new user
    #[cfg(feature = "database")]
    pub async fn create(&self, user: &CreateUser) -> Result<User, DatabaseError> {
        use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
        use rand::rngs::OsRng;

        // Hash password
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(user.password.as_bytes(), &salt)
            .map_err(|e| DatabaseError::InvalidData(e.to_string()))?
            .to_string();

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

    /// Find user by ID
    #[cfg(feature = "database")]
    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, DatabaseError> {
        let result = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(result)
    }

    /// Find user by username
    #[cfg(feature = "database")]
    pub async fn find_by_username(&self, username: &str) -> Result<Option<User>, DatabaseError> {
        let result = sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = $1")
            .bind(username)
            .fetch_optional(&self.pool)
            .await?;
        Ok(result)
    }

    /// Verify password and return user if valid
    #[cfg(feature = "database")]
    pub async fn verify_credentials(
        &self,
        username: &str,
        password: &str,
    ) -> Result<Option<User>, DatabaseError> {
        use argon2::{Argon2, PasswordHash, PasswordVerifier};

        let user = match self.find_by_username(username).await? {
            Some(u) => u,
            None => return Ok(None),
        };

        // Check if account is locked or inactive
        if user.is_locked || !user.is_active {
            return Ok(None);
        }

        // Verify password
        let parsed_hash = PasswordHash::new(&user.password_hash)
            .map_err(|e| DatabaseError::InvalidData(e.to_string()))?;

        if Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok()
        {
            // Update last login and reset failed attempts
            sqlx::query(
                "UPDATE users SET last_login = $1, failed_login_attempts = 0 WHERE id = $2",
            )
            .bind(Utc::now())
            .bind(user.id)
            .execute(&self.pool)
            .await?;

            Ok(Some(user))
        } else {
            // Increment failed attempts
            sqlx::query(
                "UPDATE users SET failed_login_attempts = failed_login_attempts + 1, 
                 is_locked = (failed_login_attempts >= 5) WHERE id = $1",
            )
            .bind(user.id)
            .execute(&self.pool)
            .await?;

            Ok(None)
        }
    }

    /// List all users with pagination
    #[cfg(feature = "database")]
    pub async fn list(
        &self,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<User>, i64), DatabaseError> {
        let users = sqlx::query_as::<_, User>(
            "SELECT * FROM users ORDER BY created_at DESC OFFSET $1 LIMIT $2",
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

    /// Update user
    #[cfg(feature = "database")]
    pub async fn update(&self, id: Uuid, update: &UpdateUser) -> Result<User, DatabaseError> {
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

    /// Delete user
    #[cfg(feature = "database")]
    pub async fn delete(&self, id: Uuid) -> Result<(), DatabaseError> {
        let result = sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(DatabaseError::NotFound);
        }

        Ok(())
    }

    /// Change user password
    #[cfg(feature = "database")]
    pub async fn change_password(
        &self,
        id: Uuid,
        new_password: &str,
    ) -> Result<(), DatabaseError> {
        use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
        use rand::rngs::OsRng;

        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(new_password.as_bytes(), &salt)
            .map_err(|e| DatabaseError::InvalidData(e.to_string()))?
            .to_string();

        sqlx::query(
            "UPDATE users SET password_hash = $1, last_password_change = $2, updated_at = $2 WHERE id = $3",
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
// Session Repository
// ============================================================================

pub struct SessionRepository {
    #[cfg(feature = "database")]
    pool: PgPool,
}

impl SessionRepository {
    #[cfg(feature = "database")]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Create a new session
    #[cfg(feature = "database")]
    pub async fn create(&self, session: &CreateSession) -> Result<DbSession, DatabaseError> {
        use sha2::{Digest, Sha256};

        let id = Uuid::new_v4();
        let now = Utc::now();

        // Hash the token for storage
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

    /// Find session by token (verifies hash)
    #[cfg(feature = "database")]
    pub async fn find_by_token(&self, token: &str) -> Result<Option<DbSession>, DatabaseError> {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        let token_hash = hex::encode(hasher.finalize());

        let result = sqlx::query_as::<_, DbSession>(
            "SELECT * FROM sessions WHERE token_hash = $1 AND is_revoked = false AND expires_at > $2",
        )
        .bind(&token_hash)
        .bind(Utc::now())
        .fetch_optional(&self.pool)
        .await?;

        Ok(result)
    }

    /// Update last activity
    #[cfg(feature = "database")]
    pub async fn touch(&self, id: Uuid) -> Result<(), DatabaseError> {
        sqlx::query("UPDATE sessions SET last_activity = $1 WHERE id = $2")
            .bind(Utc::now())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Revoke session
    #[cfg(feature = "database")]
    pub async fn revoke(&self, id: Uuid) -> Result<(), DatabaseError> {
        sqlx::query("UPDATE sessions SET is_revoked = true WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Revoke all sessions for user
    #[cfg(feature = "database")]
    pub async fn revoke_all_for_user(&self, user_id: Uuid) -> Result<u64, DatabaseError> {
        let result = sqlx::query("UPDATE sessions SET is_revoked = true WHERE user_id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    /// Cleanup expired sessions
    #[cfg(feature = "database")]
    pub async fn cleanup_expired(&self) -> Result<u64, DatabaseError> {
        let result = sqlx::query("DELETE FROM sessions WHERE expires_at < $1 OR is_revoked = true")
            .bind(Utc::now())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}

// ============================================================================
// Audit Log Repository
// ============================================================================

pub struct AuditLogRepository {
    #[cfg(feature = "database")]
    pool: PgPool,
}

impl AuditLogRepository {
    #[cfg(feature = "database")]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Create audit log entry
    #[cfg(feature = "database")]
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

    /// Search audit logs with filters
    #[cfg(feature = "database")]
    pub async fn search(
        &self,
        user_id: Option<Uuid>,
        action: Option<&str>,
        resource_type: Option<&str>,
        from: Option<chrono::DateTime<Utc>>,
        to: Option<chrono::DateTime<Utc>>,
        offset: i64,
        limit: i64,
    ) -> Result<(Vec<AuditLog>, i64), DatabaseError> {
        // Build dynamic query
        let mut conditions = vec!["1=1".to_string()];
        let mut param_idx = 1;

        if user_id.is_some() {
            conditions.push(format!("user_id = ${}", param_idx));
            param_idx += 1;
        }
        if action.is_some() {
            conditions.push(format!("action LIKE ${}", param_idx));
            param_idx += 1;
        }
        if resource_type.is_some() {
            conditions.push(format!("resource_type = ${}", param_idx));
            param_idx += 1;
        }
        if from.is_some() {
            conditions.push(format!("timestamp >= ${}", param_idx));
            param_idx += 1;
        }
        if to.is_some() {
            conditions.push(format!("timestamp <= ${}", param_idx));
            param_idx += 1;
        }

        let where_clause = conditions.join(" AND ");
        let query = format!(
            "SELECT * FROM audit_logs WHERE {} ORDER BY timestamp DESC OFFSET ${} LIMIT ${}",
            where_clause,
            param_idx,
            param_idx + 1
        );

        let mut query_builder = sqlx::query_as::<_, AuditLog>(&query);

        if let Some(uid) = user_id {
            query_builder = query_builder.bind(uid);
        }
        if let Some(a) = action {
            query_builder = query_builder.bind(format!("%{}%", a));
        }
        if let Some(rt) = resource_type {
            query_builder = query_builder.bind(rt);
        }
        if let Some(f) = from {
            query_builder = query_builder.bind(f);
        }
        if let Some(t) = to {
            query_builder = query_builder.bind(t);
        }

        let logs = query_builder
            .bind(offset)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;

        // Get total count (simplified - in production use separate count query)
        let count_query = format!("SELECT COUNT(*) FROM audit_logs WHERE {}", where_clause);
        let count: (i64,) = sqlx::query_as(&count_query)
            .fetch_one(&self.pool)
            .await
            .unwrap_or((0,));

        Ok((logs, count.0))
    }
}

// ============================================================================
// Settings Repository
// ============================================================================

pub struct SettingsRepository {
    #[cfg(feature = "database")]
    pool: PgPool,
}

impl SettingsRepository {
    #[cfg(feature = "database")]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Get setting by key
    #[cfg(feature = "database")]
    pub async fn get(&self, key: &str) -> Result<Option<Setting>, DatabaseError> {
        let result = sqlx::query_as::<_, Setting>("SELECT * FROM settings WHERE key = $1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(result)
    }

    /// Set setting value
    #[cfg(feature = "database")]
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

    /// Get all settings by category
    #[cfg(feature = "database")]
    pub async fn get_by_category(&self, category: &str) -> Result<Vec<Setting>, DatabaseError> {
        let results = sqlx::query_as::<_, Setting>(
            "SELECT * FROM settings WHERE category = $1 ORDER BY key",
        )
        .bind(category)
        .fetch_all(&self.pool)
        .await?;
        Ok(results)
    }
}
