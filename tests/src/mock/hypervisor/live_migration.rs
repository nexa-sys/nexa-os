//! Live Migration Framework
//!
//! This module provides live VM migration capabilities similar to VMware vMotion
//! or KVM live migration. Supports pre-copy and post-copy migration strategies.

use std::collections::HashMap;
use std::sync::{Arc, RwLock, Mutex, atomic::{AtomicU64, AtomicBool, Ordering}};
use std::time::{Instant, Duration};
use std::io::{Read, Write};

use super::core::{VmId, VmStatus, HypervisorError, HypervisorResult};

// ============================================================================
// Migration Manager
// ============================================================================

/// Central migration manager
pub struct MigrationManager {
    /// Active migrations
    active_migrations: RwLock<HashMap<MigrationId, Arc<Migration>>>,
    /// Migration history
    history: RwLock<Vec<MigrationRecord>>,
    /// Configuration
    config: RwLock<MigrationConfig>,
    /// Statistics
    stats: RwLock<MigrationStats>,
    /// ID generator
    next_id: AtomicU64,
}

/// Unique migration identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MigrationId(u64);

impl MigrationId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
    
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for MigrationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "mig-{:08x}", self.0)
    }
}

/// Migration configuration
#[derive(Debug, Clone)]
pub struct MigrationConfig {
    /// Maximum bandwidth (bytes/sec, 0 = unlimited)
    pub max_bandwidth: u64,
    /// Downtime threshold (milliseconds)
    pub max_downtime_ms: u64,
    /// Compression enabled
    pub compression: bool,
    /// Encryption enabled
    pub encryption: bool,
    /// Auto-converge (slow down vCPUs to help migration converge)
    pub auto_converge: bool,
    /// XBZRLE compression (for repeated pages)
    pub xbzrle: bool,
    /// Multifd (multiple channels for parallel transfer)
    pub multifd: bool,
    /// Number of multifd channels
    pub multifd_channels: u32,
    /// Zero page detection
    pub zero_page_detection: bool,
    /// Post-copy migration threshold (pages remaining)
    pub postcopy_threshold: u64,
}

impl Default for MigrationConfig {
    fn default() -> Self {
        Self {
            max_bandwidth: 0, // Unlimited
            max_downtime_ms: 300,
            compression: true,
            encryption: true,
            auto_converge: true,
            xbzrle: true,
            multifd: true,
            multifd_channels: 4,
            zero_page_detection: true,
            postcopy_threshold: 1000,
        }
    }
}

/// Migration statistics
#[derive(Debug, Clone, Default)]
pub struct MigrationStats {
    pub total_migrations: u64,
    pub successful_migrations: u64,
    pub failed_migrations: u64,
    pub total_bytes_transferred: u64,
    pub total_pages_transferred: u64,
    pub average_downtime_ms: u64,
    pub average_duration_secs: u64,
}

/// Migration record (history)
#[derive(Debug, Clone)]
pub struct MigrationRecord {
    pub id: MigrationId,
    pub vm_id: VmId,
    pub source_host: String,
    pub dest_host: String,
    pub started_at: Instant,
    pub completed_at: Option<Instant>,
    pub status: MigrationState,
    pub bytes_transferred: u64,
    pub downtime_ms: u64,
}

impl MigrationManager {
    pub fn new() -> Self {
        Self {
            active_migrations: RwLock::new(HashMap::new()),
            history: RwLock::new(Vec::new()),
            config: RwLock::new(MigrationConfig::default()),
            stats: RwLock::new(MigrationStats::default()),
            next_id: AtomicU64::new(1),
        }
    }
    
    /// Configure migration settings
    pub fn configure(&self, config: MigrationConfig) {
        *self.config.write().unwrap() = config;
    }
    
    /// Start a live migration
    pub fn start_migration(
        &self,
        vm_id: VmId,
        source_host: &str,
        dest_host: &str,
        options: MigrationOptions,
    ) -> HypervisorResult<MigrationId> {
        let id = MigrationId::new(self.next_id.fetch_add(1, Ordering::SeqCst));
        let config = self.config.read().unwrap().clone();
        
        let migration = Arc::new(Migration::new(
            id,
            vm_id,
            source_host.to_string(),
            dest_host.to_string(),
            options,
            config,
        ));
        
        self.active_migrations.write().unwrap().insert(id, migration.clone());
        
        // Start migration in background
        migration.start()?;
        
        self.stats.write().unwrap().total_migrations += 1;
        
        Ok(id)
    }
    
    /// Cancel an active migration
    pub fn cancel_migration(&self, id: MigrationId) -> HypervisorResult<()> {
        let migration = self.get_migration(id)?;
        migration.cancel()?;
        
        // Move to history
        self.finalize_migration(id, false);
        
        Ok(())
    }
    
    /// Get migration progress
    pub fn get_progress(&self, id: MigrationId) -> HypervisorResult<MigrationProgress> {
        let migration = self.get_migration(id)?;
        Ok(migration.progress())
    }
    
    /// Get migration state
    pub fn get_state(&self, id: MigrationId) -> HypervisorResult<MigrationState> {
        let migration = self.get_migration(id)?;
        Ok(migration.state())
    }
    
    /// Wait for migration to complete
    pub fn wait_for_completion(&self, id: MigrationId, timeout: Duration) -> HypervisorResult<bool> {
        let migration = self.get_migration(id)?;
        let start = Instant::now();
        
        while start.elapsed() < timeout {
            match migration.state() {
                MigrationState::Completed => {
                    self.finalize_migration(id, true);
                    return Ok(true);
                }
                MigrationState::Failed(_) => {
                    self.finalize_migration(id, false);
                    return Ok(false);
                }
                MigrationState::Cancelled => {
                    self.finalize_migration(id, false);
                    return Ok(false);
                }
                _ => {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
        
        Ok(false)
    }
    
    /// List active migrations
    pub fn list_active(&self) -> Vec<(MigrationId, VmId, MigrationState)> {
        self.active_migrations.read().unwrap()
            .iter()
            .map(|(id, m)| (*id, m.vm_id, m.state()))
            .collect()
    }
    
    /// Get migration history
    pub fn get_history(&self, limit: usize) -> Vec<MigrationRecord> {
        let history = self.history.read().unwrap();
        history.iter().rev().take(limit).cloned().collect()
    }
    
    /// Get statistics
    pub fn stats(&self) -> MigrationStats {
        self.stats.read().unwrap().clone()
    }
    
    fn get_migration(&self, id: MigrationId) -> HypervisorResult<Arc<Migration>> {
        self.active_migrations.read().unwrap()
            .get(&id)
            .cloned()
            .ok_or(HypervisorError::MigrationError(
                format!("Migration {} not found", id)
            ))
    }
    
    fn finalize_migration(&self, id: MigrationId, success: bool) {
        if let Some(migration) = self.active_migrations.write().unwrap().remove(&id) {
            let record = MigrationRecord {
                id,
                vm_id: migration.vm_id,
                source_host: migration.source_host.clone(),
                dest_host: migration.dest_host.clone(),
                started_at: migration.started_at,
                completed_at: Some(Instant::now()),
                status: migration.state(),
                bytes_transferred: migration.progress().bytes_transferred,
                downtime_ms: migration.progress().downtime_ms,
            };
            
            self.history.write().unwrap().push(record);
            
            let mut stats = self.stats.write().unwrap();
            if success {
                stats.successful_migrations += 1;
            } else {
                stats.failed_migrations += 1;
            }
            stats.total_bytes_transferred += migration.progress().bytes_transferred;
        }
    }
}

impl Default for MigrationManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Migration Instance
// ============================================================================

/// Individual migration instance
pub struct Migration {
    id: MigrationId,
    vm_id: VmId,
    source_host: String,
    dest_host: String,
    options: MigrationOptions,
    config: MigrationConfig,
    started_at: Instant,
    state: RwLock<MigrationState>,
    progress: RwLock<MigrationProgress>,
    cancelled: AtomicBool,
}

impl Migration {
    pub fn new(
        id: MigrationId,
        vm_id: VmId,
        source_host: String,
        dest_host: String,
        options: MigrationOptions,
        config: MigrationConfig,
    ) -> Self {
        Self {
            id,
            vm_id,
            source_host,
            dest_host,
            options,
            config,
            started_at: Instant::now(),
            state: RwLock::new(MigrationState::Pending),
            progress: RwLock::new(MigrationProgress::default()),
            cancelled: AtomicBool::new(false),
        }
    }
    
    /// Start the migration
    pub fn start(&self) -> HypervisorResult<()> {
        *self.state.write().unwrap() = MigrationState::Setup;
        
        // Simulate migration phases
        match self.options.migration_type {
            MigrationType::PreCopy => self.run_precopy_migration(),
            MigrationType::PostCopy => self.run_postcopy_migration(),
            MigrationType::Hybrid => self.run_hybrid_migration(),
            MigrationType::Offline => self.run_offline_migration(),
        }
    }
    
    /// Cancel the migration
    pub fn cancel(&self) -> HypervisorResult<()> {
        self.cancelled.store(true, Ordering::SeqCst);
        *self.state.write().unwrap() = MigrationState::Cancelled;
        Ok(())
    }
    
    /// Get current state
    pub fn state(&self) -> MigrationState {
        self.state.read().unwrap().clone()
    }
    
    /// Get progress
    pub fn progress(&self) -> MigrationProgress {
        self.progress.read().unwrap().clone()
    }
    
    /// Run pre-copy migration (default live migration)
    fn run_precopy_migration(&self) -> HypervisorResult<()> {
        // Phase 1: Setup
        *self.state.write().unwrap() = MigrationState::Setup;
        self.check_cancelled()?;
        
        // Phase 2: Transfer memory (iterative)
        *self.state.write().unwrap() = MigrationState::TransferMemory;
        
        // Simulate memory transfer iterations
        let total_pages = 262144u64; // ~1GB
        let mut pages_transferred = 0u64;
        let mut iteration = 0;
        
        while pages_transferred < total_pages && iteration < 10 {
            self.check_cancelled()?;
            
            // Simulate dirty pages (decreasing each iteration)
            let dirty_pages = total_pages / (2u64.pow(iteration as u32)).max(1);
            pages_transferred += dirty_pages;
            
            // Update progress
            {
                let mut progress = self.progress.write().unwrap();
                progress.total_pages = total_pages;
                progress.transferred_pages = pages_transferred;
                progress.dirty_pages_rate = dirty_pages;
                progress.iteration = iteration;
                progress.bytes_transferred = pages_transferred * 4096;
            }
            
            iteration += 1;
            
            // Check if we're close enough for final switchover
            let remaining = total_pages - pages_transferred;
            if remaining < self.config.postcopy_threshold {
                break;
            }
        }
        
        // Phase 3: Switchover (stop-and-copy)
        *self.state.write().unwrap() = MigrationState::Switchover;
        self.check_cancelled()?;
        
        // Simulate downtime
        let downtime_start = Instant::now();
        std::thread::sleep(Duration::from_millis(self.config.max_downtime_ms));
        
        // Transfer remaining pages
        {
            let mut progress = self.progress.write().unwrap();
            progress.transferred_pages = total_pages;
            progress.downtime_ms = downtime_start.elapsed().as_millis() as u64;
        }
        
        // Phase 4: Complete
        *self.state.write().unwrap() = MigrationState::Completed;
        
        Ok(())
    }
    
    /// Run post-copy migration
    fn run_postcopy_migration(&self) -> HypervisorResult<()> {
        // Phase 1: Setup
        *self.state.write().unwrap() = MigrationState::Setup;
        self.check_cancelled()?;
        
        // Phase 2: Transfer minimal state and switch immediately
        *self.state.write().unwrap() = MigrationState::Switchover;
        
        // Minimal downtime - just transfer CPU state
        let downtime_start = Instant::now();
        std::thread::sleep(Duration::from_millis(10));
        
        {
            let mut progress = self.progress.write().unwrap();
            progress.downtime_ms = downtime_start.elapsed().as_millis() as u64;
        }
        
        // Phase 3: Post-copy memory fetch
        *self.state.write().unwrap() = MigrationState::PostCopy;
        
        // Simulate demand paging
        let total_pages = 262144u64;
        let mut progress = self.progress.write().unwrap();
        progress.total_pages = total_pages;
        progress.transferred_pages = 0;
        drop(progress);
        
        // Pages are transferred on-demand
        // Simulate gradual transfer
        for _ in 0..10 {
            self.check_cancelled()?;
            std::thread::sleep(Duration::from_millis(50));
            
            let mut progress = self.progress.write().unwrap();
            progress.transferred_pages += total_pages / 10;
        }
        
        // Phase 4: Complete
        *self.state.write().unwrap() = MigrationState::Completed;
        
        Ok(())
    }
    
    /// Run hybrid migration (pre-copy + post-copy)
    fn run_hybrid_migration(&self) -> HypervisorResult<()> {
        // Start with pre-copy
        *self.state.write().unwrap() = MigrationState::Setup;
        self.check_cancelled()?;
        
        // Transfer most memory via pre-copy
        *self.state.write().unwrap() = MigrationState::TransferMemory;
        
        let total_pages = 262144u64;
        let precopy_target = total_pages * 80 / 100; // Transfer 80% via pre-copy
        
        {
            let mut progress = self.progress.write().unwrap();
            progress.total_pages = total_pages;
        }
        
        let mut transferred = 0u64;
        while transferred < precopy_target {
            self.check_cancelled()?;
            
            let chunk = (precopy_target - transferred).min(total_pages / 10);
            transferred += chunk;
            
            let mut progress = self.progress.write().unwrap();
            progress.transferred_pages = transferred;
            progress.bytes_transferred = transferred * 4096;
        }
        
        // Switchover
        *self.state.write().unwrap() = MigrationState::Switchover;
        let downtime_start = Instant::now();
        std::thread::sleep(Duration::from_millis(20));
        
        {
            let mut progress = self.progress.write().unwrap();
            progress.downtime_ms = downtime_start.elapsed().as_millis() as u64;
        }
        
        // Post-copy remaining pages
        *self.state.write().unwrap() = MigrationState::PostCopy;
        
        while transferred < total_pages {
            self.check_cancelled()?;
            
            transferred += total_pages / 20;
            transferred = transferred.min(total_pages);
            
            let mut progress = self.progress.write().unwrap();
            progress.transferred_pages = transferred;
        }
        
        *self.state.write().unwrap() = MigrationState::Completed;
        Ok(())
    }
    
    /// Run offline (cold) migration
    fn run_offline_migration(&self) -> HypervisorResult<()> {
        *self.state.write().unwrap() = MigrationState::Setup;
        self.check_cancelled()?;
        
        // VM is already stopped, just transfer disk image
        *self.state.write().unwrap() = MigrationState::TransferDisk;
        
        let total_bytes = 10 * 1024 * 1024 * 1024u64; // 10GB disk
        let mut transferred = 0u64;
        
        while transferred < total_bytes {
            self.check_cancelled()?;
            
            transferred += 100 * 1024 * 1024; // 100MB chunks
            transferred = transferred.min(total_bytes);
            
            let mut progress = self.progress.write().unwrap();
            progress.bytes_transferred = transferred;
        }
        
        *self.state.write().unwrap() = MigrationState::Completed;
        Ok(())
    }
    
    fn check_cancelled(&self) -> HypervisorResult<()> {
        if self.cancelled.load(Ordering::SeqCst) {
            return Err(HypervisorError::MigrationError("Migration cancelled".to_string()));
        }
        Ok(())
    }
}

// ============================================================================
// Migration Types and Options
// ============================================================================

/// Migration type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationType {
    /// Pre-copy: transfer memory, then switch (VMware vMotion style)
    PreCopy,
    /// Post-copy: switch immediately, fetch pages on demand
    PostCopy,
    /// Hybrid: pre-copy most pages, post-copy remainder
    Hybrid,
    /// Offline: cold migration (VM stopped)
    Offline,
}

impl Default for MigrationType {
    fn default() -> Self {
        Self::PreCopy
    }
}

/// Migration options
#[derive(Debug, Clone)]
pub struct MigrationOptions {
    /// Migration type
    pub migration_type: MigrationType,
    /// Maximum bandwidth (bytes/sec, 0 = unlimited)
    pub bandwidth_limit: u64,
    /// Enable compression
    pub compression: bool,
    /// Enable encryption
    pub encryption: bool,
    /// Auto-converge threshold (CPU throttle)
    pub auto_converge_threshold: Option<f64>,
    /// Pause on postcopy failure
    pub pause_on_postcopy_fail: bool,
    /// Resume VM on destination
    pub auto_resume: bool,
    /// Timeout (seconds)
    pub timeout: u64,
}

impl Default for MigrationOptions {
    fn default() -> Self {
        Self {
            migration_type: MigrationType::PreCopy,
            bandwidth_limit: 0,
            compression: true,
            encryption: true,
            auto_converge_threshold: Some(0.5),
            pause_on_postcopy_fail: true,
            auto_resume: true,
            timeout: 3600,
        }
    }
}

impl MigrationOptions {
    /// Create live migration options (pre-copy)
    pub fn live() -> Self {
        Self::default()
    }
    
    /// Create post-copy migration options
    pub fn postcopy() -> Self {
        Self {
            migration_type: MigrationType::PostCopy,
            ..Self::default()
        }
    }
    
    /// Create hybrid migration options
    pub fn hybrid() -> Self {
        Self {
            migration_type: MigrationType::Hybrid,
            ..Self::default()
        }
    }
    
    /// Create offline migration options
    pub fn offline() -> Self {
        Self {
            migration_type: MigrationType::Offline,
            compression: true,
            encryption: true,
            auto_resume: false,
            ..Self::default()
        }
    }
    
    pub fn with_bandwidth(mut self, limit: u64) -> Self {
        self.bandwidth_limit = limit;
        self
    }
    
    pub fn with_timeout(mut self, seconds: u64) -> Self {
        self.timeout = seconds;
        self
    }
}

/// Migration state
#[derive(Debug, Clone, PartialEq)]
pub enum MigrationState {
    /// Migration pending
    Pending,
    /// Setting up migration
    Setup,
    /// Transferring memory (pre-copy)
    TransferMemory,
    /// Transferring disk (offline)
    TransferDisk,
    /// Switchover (stop-and-copy)
    Switchover,
    /// Post-copy phase
    PostCopy,
    /// Migration completed
    Completed,
    /// Migration failed
    Failed(String),
    /// Migration cancelled
    Cancelled,
}

impl std::fmt::Display for MigrationState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Setup => write!(f, "setup"),
            Self::TransferMemory => write!(f, "transfer-memory"),
            Self::TransferDisk => write!(f, "transfer-disk"),
            Self::Switchover => write!(f, "switchover"),
            Self::PostCopy => write!(f, "post-copy"),
            Self::Completed => write!(f, "completed"),
            Self::Failed(msg) => write!(f, "failed: {}", msg),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Migration progress
#[derive(Debug, Clone, Default)]
pub struct MigrationProgress {
    /// Total pages to transfer
    pub total_pages: u64,
    /// Pages transferred
    pub transferred_pages: u64,
    /// Current dirty pages rate (pages/iteration)
    pub dirty_pages_rate: u64,
    /// Current iteration (pre-copy)
    pub iteration: u32,
    /// Bytes transferred
    pub bytes_transferred: u64,
    /// Transfer rate (bytes/sec)
    pub transfer_rate: u64,
    /// Estimated time remaining (seconds)
    pub eta_seconds: u64,
    /// Downtime (milliseconds)
    pub downtime_ms: u64,
    /// Compression ratio
    pub compression_ratio: f64,
    /// Pages skipped (zero pages)
    pub pages_skipped: u64,
}

impl MigrationProgress {
    /// Get completion percentage
    pub fn completion_percent(&self) -> f64 {
        if self.total_pages == 0 {
            return 0.0;
        }
        (self.transferred_pages as f64 / self.total_pages as f64) * 100.0
    }
    
    /// Get remaining pages
    pub fn remaining_pages(&self) -> u64 {
        self.total_pages.saturating_sub(self.transferred_pages)
    }
}

// ============================================================================
// Pre-Copy Migration Implementation
// ============================================================================

/// Pre-copy migration implementation
pub struct PreCopyMigration {
    /// Configuration
    config: MigrationConfig,
    /// Memory bitmap (dirty tracking)
    dirty_bitmap: RwLock<Vec<bool>>,
    /// Page hashes (for XBZRLE)
    page_hashes: RwLock<HashMap<u64, u64>>,
    /// Statistics
    stats: RwLock<PreCopyStats>,
}

#[derive(Debug, Clone, Default)]
pub struct PreCopyStats {
    pub iterations: u32,
    pub pages_sent: u64,
    pub pages_skipped: u64,
    pub xbzrle_hits: u64,
    pub xbzrle_misses: u64,
    pub zero_pages: u64,
}

impl PreCopyMigration {
    pub fn new(config: MigrationConfig, total_pages: u64) -> Self {
        Self {
            config,
            dirty_bitmap: RwLock::new(vec![true; total_pages as usize]),
            page_hashes: RwLock::new(HashMap::new()),
            stats: RwLock::new(PreCopyStats::default()),
        }
    }
    
    /// Mark page as dirty
    pub fn mark_dirty(&self, page: u64) {
        let mut bitmap = self.dirty_bitmap.write().unwrap();
        if (page as usize) < bitmap.len() {
            bitmap[page as usize] = true;
        }
    }
    
    /// Get dirty pages for current iteration
    pub fn get_dirty_pages(&self) -> Vec<u64> {
        let bitmap = self.dirty_bitmap.read().unwrap();
        bitmap.iter()
            .enumerate()
            .filter(|(_, &dirty)| dirty)
            .map(|(i, _)| i as u64)
            .collect()
    }
    
    /// Clear dirty bitmap after iteration
    pub fn clear_dirty(&self) {
        let mut bitmap = self.dirty_bitmap.write().unwrap();
        for dirty in bitmap.iter_mut() {
            *dirty = false;
        }
    }
    
    /// Transfer a page (simulated)
    pub fn transfer_page(&self, page: u64, data: &[u8]) -> TransferResult {
        // Check for zero page
        if data.iter().all(|&b| b == 0) {
            self.stats.write().unwrap().zero_pages += 1;
            return TransferResult::ZeroPage;
        }
        
        // Calculate page hash for XBZRLE
        let hash = self.calculate_hash(data);
        
        if self.config.xbzrle {
            let hashes = self.page_hashes.read().unwrap();
            if let Some(&old_hash) = hashes.get(&page) {
                if old_hash == hash {
                    self.stats.write().unwrap().xbzrle_hits += 1;
                    return TransferResult::XbzrleHit;
                }
            }
            drop(hashes);
            
            // Update hash
            self.page_hashes.write().unwrap().insert(page, hash);
            self.stats.write().unwrap().xbzrle_misses += 1;
        }
        
        self.stats.write().unwrap().pages_sent += 1;
        TransferResult::Sent(data.len())
    }
    
    fn calculate_hash(&self, data: &[u8]) -> u64 {
        // Simple hash for testing
        let mut hash = 0u64;
        for &byte in data {
            hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
        }
        hash
    }
    
    /// Get statistics
    pub fn stats(&self) -> PreCopyStats {
        self.stats.read().unwrap().clone()
    }
}

/// Page transfer result
#[derive(Debug, Clone, Copy)]
pub enum TransferResult {
    /// Page was sent
    Sent(usize),
    /// Page was zero (not sent)
    ZeroPage,
    /// XBZRLE hit (not sent)
    XbzrleHit,
    /// XBZRLE delta sent
    XbzrleDelta(usize),
}

// ============================================================================
// Post-Copy Migration Implementation
// ============================================================================

/// Post-copy migration implementation
pub struct PostCopyMigration {
    /// Configuration
    config: MigrationConfig,
    /// Page presence bitmap
    present_bitmap: RwLock<Vec<bool>>,
    /// Pending page requests
    pending_requests: RwLock<Vec<u64>>,
    /// Statistics
    stats: RwLock<PostCopyStats>,
}

#[derive(Debug, Clone, Default)]
pub struct PostCopyStats {
    pub pages_fetched: u64,
    pub page_faults: u64,
    pub prefetch_hits: u64,
    pub prefetch_misses: u64,
}

impl PostCopyMigration {
    pub fn new(config: MigrationConfig, total_pages: u64) -> Self {
        Self {
            config,
            present_bitmap: RwLock::new(vec![false; total_pages as usize]),
            pending_requests: RwLock::new(Vec::new()),
            stats: RwLock::new(PostCopyStats::default()),
        }
    }
    
    /// Handle page fault (page not present)
    pub fn handle_page_fault(&self, page: u64) -> bool {
        let bitmap = self.present_bitmap.read().unwrap();
        if (page as usize) < bitmap.len() && bitmap[page as usize] {
            return true; // Page already present
        }
        drop(bitmap);
        
        // Queue request
        self.pending_requests.write().unwrap().push(page);
        self.stats.write().unwrap().page_faults += 1;
        
        false
    }
    
    /// Mark page as present
    pub fn mark_present(&self, page: u64) {
        let mut bitmap = self.present_bitmap.write().unwrap();
        if (page as usize) < bitmap.len() {
            bitmap[page as usize] = true;
        }
        self.stats.write().unwrap().pages_fetched += 1;
    }
    
    /// Get pending page requests
    pub fn get_pending_requests(&self) -> Vec<u64> {
        let mut requests = self.pending_requests.write().unwrap();
        std::mem::take(&mut *requests)
    }
    
    /// Prefetch pages (background transfer)
    pub fn prefetch(&self, pages: &[u64]) {
        let mut bitmap = self.present_bitmap.write().unwrap();
        let mut stats = self.stats.write().unwrap();
        
        for &page in pages {
            if (page as usize) < bitmap.len() && !bitmap[page as usize] {
                bitmap[page as usize] = true;
                stats.prefetch_hits += 1;
            }
        }
    }
    
    /// Get statistics
    pub fn stats(&self) -> PostCopyStats {
        self.stats.read().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_migration_manager() {
        let manager = MigrationManager::new();
        
        let id = manager.start_migration(
            VmId::new(1),
            "host1",
            "host2",
            MigrationOptions::live(),
        ).unwrap();
        
        assert!(manager.list_active().len() > 0);
        
        // Wait for completion (with timeout)
        let completed = manager.wait_for_completion(id, Duration::from_secs(10)).unwrap();
        assert!(completed);
        
        assert_eq!(manager.list_active().len(), 0);
        assert!(manager.get_history(10).len() > 0);
    }
    
    #[test]
    fn test_precopy_migration() {
        let config = MigrationConfig::default();
        let migration = PreCopyMigration::new(config, 1000);
        
        // Mark some pages dirty
        for i in 0..100 {
            migration.mark_dirty(i);
        }
        
        let dirty = migration.get_dirty_pages();
        assert_eq!(dirty.len(), 1000); // All pages dirty initially
        
        migration.clear_dirty();
        let dirty = migration.get_dirty_pages();
        assert_eq!(dirty.len(), 0);
    }
    
    #[test]
    fn test_postcopy_migration() {
        let config = MigrationConfig::default();
        let migration = PostCopyMigration::new(config, 1000);
        
        // Page fault
        assert!(!migration.handle_page_fault(42));
        
        let pending = migration.get_pending_requests();
        assert_eq!(pending, vec![42]);
        
        // Mark present
        migration.mark_present(42);
        assert!(migration.handle_page_fault(42)); // Now present
    }
    
    #[test]
    fn test_migration_options() {
        let live = MigrationOptions::live();
        assert_eq!(live.migration_type, MigrationType::PreCopy);
        
        let postcopy = MigrationOptions::postcopy();
        assert_eq!(postcopy.migration_type, MigrationType::PostCopy);
        
        let offline = MigrationOptions::offline();
        assert_eq!(offline.migration_type, MigrationType::Offline);
        assert!(!offline.auto_resume);
    }
}
