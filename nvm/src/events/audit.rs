//! Audit Logger - Compliance-grade audit logging

use super::{Event, EventCategory, EventSeverity};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

/// Audit log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Entry ID
    pub id: u64,
    /// Timestamp (Unix epoch)
    pub timestamp: u64,
    /// Action performed
    pub action: String,
    /// User who performed the action
    pub user: String,
    /// Source IP address
    pub source_ip: Option<String>,
    /// Target resource type
    pub resource_type: String,
    /// Target resource ID
    pub resource_id: String,
    /// Result (success/failure)
    pub result: AuditResult,
    /// Additional details
    pub details: HashMap<String, String>,
    /// Session ID
    pub session_id: Option<String>,
}

/// Audit result
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditResult {
    Success,
    Failure,
    Denied,
}

/// Audit logger configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditConfig {
    /// Enable audit logging
    pub enabled: bool,
    /// Log file path
    pub log_path: PathBuf,
    /// Log to syslog
    pub syslog: bool,
    /// Retain logs for days
    pub retention_days: u32,
    /// Actions to audit (empty = all)
    pub audit_actions: Vec<String>,
    /// Log format (json/text)
    pub format: AuditFormat,
}

/// Audit log format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditFormat {
    Json,
    Text,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            log_path: PathBuf::from("/var/log/nvm/audit.log"),
            syslog: false,
            retention_days: 90,
            audit_actions: Vec::new(),
            format: AuditFormat::Json,
        }
    }
}

/// Audit logger
pub struct AuditLogger {
    config: RwLock<AuditConfig>,
    writer: RwLock<Option<BufWriter<File>>>,
    entry_counter: std::sync::atomic::AtomicU64,
}

impl AuditLogger {
    /// Create new audit logger
    pub fn new(config: AuditConfig) -> Self {
        let writer = if config.enabled {
            Self::open_log_file(&config.log_path).ok()
        } else {
            None
        };

        Self {
            config: RwLock::new(config),
            writer: RwLock::new(writer),
            entry_counter: std::sync::atomic::AtomicU64::new(1),
        }
    }

    fn open_log_file(path: &PathBuf) -> std::io::Result<BufWriter<File>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        
        Ok(BufWriter::new(file))
    }

    /// Log an audit entry
    pub fn log(&self, entry: AuditEntry) {
        let config = self.config.read();
        
        if !config.enabled {
            return;
        }

        // Check if action should be audited
        if !config.audit_actions.is_empty() {
            if !config.audit_actions.iter().any(|a| entry.action.starts_with(a)) {
                return;
            }
        }

        let formatted = match config.format {
            AuditFormat::Json => {
                serde_json::to_string(&entry).unwrap_or_default()
            }
            AuditFormat::Text => {
                format!(
                    "{} {} {} {} {} {} {}",
                    entry.timestamp,
                    entry.user,
                    entry.action,
                    entry.resource_type,
                    entry.resource_id,
                    match entry.result {
                        AuditResult::Success => "SUCCESS",
                        AuditResult::Failure => "FAILURE",
                        AuditResult::Denied => "DENIED",
                    },
                    entry.source_ip.unwrap_or_default()
                )
            }
        };

        drop(config);

        // Write to file
        if let Some(ref mut writer) = *self.writer.write() {
            let _ = writeln!(writer, "{}", formatted);
            let _ = writer.flush();
        }
    }

    /// Create audit entry builder
    pub fn entry(&self, action: impl Into<String>, user: impl Into<String>) -> AuditEntryBuilder {
        let id = self.entry_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        
        AuditEntryBuilder {
            entry: AuditEntry {
                id,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                action: action.into(),
                user: user.into(),
                source_ip: None,
                resource_type: String::new(),
                resource_id: String::new(),
                result: AuditResult::Success,
                details: HashMap::new(),
                session_id: None,
            },
        }
    }

    /// Log from event
    pub fn log_event(&self, event: &Event) {
        let entry = AuditEntry {
            id: event.id,
            timestamp: event.timestamp,
            action: event.event_type.clone(),
            user: event.user.clone().unwrap_or_else(|| "system".to_string()),
            source_ip: event.metadata.get("source_ip").cloned(),
            resource_type: event.resource_type.clone().unwrap_or_default(),
            resource_id: event.resource_id.clone().unwrap_or_default(),
            result: AuditResult::Success,
            details: event.metadata.clone(),
            session_id: event.metadata.get("session_id").cloned(),
        };

        self.log(entry);
    }

    /// Rotate log files
    pub fn rotate(&self) -> std::io::Result<()> {
        let config = self.config.read();
        let path = config.log_path.clone();
        drop(config);

        // Close current writer
        *self.writer.write() = None;

        // Rename current log
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let rotated_name = format!("{}.{}", path.display(), timestamp);
        std::fs::rename(&path, &rotated_name)?;

        // Open new log file
        let config = self.config.read();
        *self.writer.write() = Self::open_log_file(&config.log_path).ok();

        Ok(())
    }

    /// Query audit logs
    pub fn query(&self, filter: AuditQuery) -> Vec<AuditEntry> {
        // In production, this would query from database/file
        Vec::new()
    }
}

/// Audit entry builder
pub struct AuditEntryBuilder {
    entry: AuditEntry,
}

impl AuditEntryBuilder {
    pub fn resource(mut self, resource_type: impl Into<String>, resource_id: impl Into<String>) -> Self {
        self.entry.resource_type = resource_type.into();
        self.entry.resource_id = resource_id.into();
        self
    }

    pub fn source_ip(mut self, ip: impl Into<String>) -> Self {
        self.entry.source_ip = Some(ip.into());
        self
    }

    pub fn session(mut self, session_id: impl Into<String>) -> Self {
        self.entry.session_id = Some(session_id.into());
        self
    }

    pub fn result(mut self, result: AuditResult) -> Self {
        self.entry.result = result;
        self
    }

    pub fn detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.entry.details.insert(key.into(), value.into());
        self
    }

    pub fn build(self) -> AuditEntry {
        self.entry
    }
}

/// Audit query parameters
#[derive(Debug, Clone, Default)]
pub struct AuditQuery {
    pub start_time: Option<u64>,
    pub end_time: Option<u64>,
    pub user: Option<String>,
    pub action: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub result: Option<AuditResult>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new(AuditConfig::default())
    }
}
