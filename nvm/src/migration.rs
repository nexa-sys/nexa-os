//! Live Migration
//!
//! VM live migration features including:
//! - Pre-copy memory migration
//! - Post-copy migration
//! - Storage migration
//! - Cross-cluster migration

use std::collections::HashMap;
use std::sync::{Arc, RwLock, atomic::{AtomicU64, Ordering}};
use std::time::Instant;

use crate::hypervisor::VmId;

/// Migration manager
pub struct MigrationManager {
    jobs: RwLock<HashMap<u64, MigrationJob>>,
    config: RwLock<MigrationConfig>,
    next_id: AtomicU64,
    stats: RwLock<MigrationStats>,
}

/// Migration configuration
#[derive(Debug, Clone)]
pub struct MigrationConfig {
    pub bandwidth_limit_mbps: u64,
    pub compression: bool,
    pub encryption: bool,
    pub max_downtime_ms: u64,
    pub convergence_timeout_s: u64,
    pub auto_converge: bool,
}

impl Default for MigrationConfig {
    fn default() -> Self {
        Self {
            bandwidth_limit_mbps: 0, // unlimited
            compression: true,
            encryption: true,
            max_downtime_ms: 500,
            convergence_timeout_s: 300,
            auto_converge: true,
        }
    }
}

/// Migration job
#[derive(Debug, Clone)]
pub struct MigrationJob {
    pub id: u64,
    pub vm_id: VmId,
    pub source_node: String,
    pub target_node: String,
    pub migration_type: MigrationType,
    pub status: MigrationStatus,
    pub progress: MigrationProgress,
    pub started_at: Option<Instant>,
    pub completed_at: Option<Instant>,
    pub error: Option<String>,
}

/// Migration type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationType {
    Live,
    Offline,
    StorageOnly,
    CrossCluster,
}

/// Migration status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationStatus {
    Pending,
    Preparing,
    PreCopy,
    StopAndCopy,
    PostCopy,
    Completed,
    Failed,
    Cancelled,
}

/// Migration progress
#[derive(Debug, Clone, Default)]
pub struct MigrationProgress {
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub dirty_rate_mbps: f64,
    pub transfer_rate_mbps: f64,
    pub remaining_bytes: u64,
    pub iterations: u32,
    pub downtime_ms: u64,
}

/// Migration statistics
#[derive(Debug, Clone, Default)]
pub struct MigrationStats {
    pub total_migrations: u64,
    pub successful: u64,
    pub failed: u64,
    pub total_bytes_transferred: u64,
    pub average_downtime_ms: f64,
}

impl MigrationManager {
    pub fn new() -> Self {
        Self {
            jobs: RwLock::new(HashMap::new()),
            config: RwLock::new(MigrationConfig::default()),
            next_id: AtomicU64::new(1),
            stats: RwLock::new(MigrationStats::default()),
        }
    }
    
    pub fn configure(&self, config: MigrationConfig) {
        *self.config.write().unwrap() = config;
    }
    
    pub fn start_migration(
        &self,
        vm_id: VmId,
        source: &str,
        target: &str,
        migration_type: MigrationType,
    ) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        
        let job = MigrationJob {
            id,
            vm_id,
            source_node: source.to_string(),
            target_node: target.to_string(),
            migration_type,
            status: MigrationStatus::Pending,
            progress: MigrationProgress::default(),
            started_at: Some(Instant::now()),
            completed_at: None,
            error: None,
        };
        
        self.jobs.write().unwrap().insert(id, job);
        id
    }
    
    pub fn get_job(&self, id: u64) -> Option<MigrationJob> {
        self.jobs.read().unwrap().get(&id).cloned()
    }
    
    pub fn cancel_migration(&self, id: u64) -> Result<(), String> {
        let mut jobs = self.jobs.write().unwrap();
        if let Some(job) = jobs.get_mut(&id) {
            match job.status {
                MigrationStatus::Pending | MigrationStatus::Preparing |
                MigrationStatus::PreCopy => {
                    job.status = MigrationStatus::Cancelled;
                    Ok(())
                }
                _ => Err("Migration cannot be cancelled at this stage".to_string())
            }
        } else {
            Err("Job not found".to_string())
        }
    }
    
    pub fn update_progress(&self, id: u64, progress: MigrationProgress) {
        if let Some(job) = self.jobs.write().unwrap().get_mut(&id) {
            job.progress = progress;
        }
    }
    
    pub fn complete_migration(&self, id: u64, success: bool, error: Option<String>) {
        let mut jobs = self.jobs.write().unwrap();
        if let Some(job) = jobs.get_mut(&id) {
            job.status = if success { MigrationStatus::Completed } else { MigrationStatus::Failed };
            job.completed_at = Some(Instant::now());
            job.error = error;
        }
        
        let mut stats = self.stats.write().unwrap();
        stats.total_migrations += 1;
        if success {
            stats.successful += 1;
        } else {
            stats.failed += 1;
        }
    }
    
    pub fn list_active_jobs(&self) -> Vec<MigrationJob> {
        self.jobs.read().unwrap()
            .values()
            .filter(|j| !matches!(j.status, MigrationStatus::Completed | MigrationStatus::Failed | MigrationStatus::Cancelled))
            .cloned()
            .collect()
    }
    
    pub fn stats(&self) -> MigrationStats {
        self.stats.read().unwrap().clone()
    }
}

impl Default for MigrationManager {
    fn default() -> Self { Self::new() }
}
