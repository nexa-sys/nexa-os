//! Virtual Storage Management
//!
//! This module provides enterprise storage features including:
//! - Virtual disk formats (QCOW2, VMDK, VDI, RAW)
//! - Snapshot chains and linked clones
//! - Storage tiering and caching
//! - Storage migration and replication
//! - Thin provisioning and deduplication

use std::collections::HashMap;
use std::sync::{Arc, RwLock, Mutex, atomic::{AtomicU64, Ordering}};
use std::time::{Instant, Duration};
use std::path::PathBuf;

use super::core::{VmId, HypervisorError, HypervisorResult};

// ============================================================================
// Storage Manager
// ============================================================================

/// Central storage manager
pub struct StorageManager {
    /// Storage pools
    pools: RwLock<HashMap<StoragePoolId, Arc<StoragePool>>>,
    /// Virtual disks
    disks: RwLock<HashMap<VirtualDiskId, Arc<VirtualDisk>>>,
    /// Snapshot chains
    snapshots: RwLock<HashMap<SnapshotId, Arc<Snapshot>>>,
    /// Storage backends
    backends: RwLock<HashMap<String, Arc<dyn StorageBackend>>>,
    /// Configuration
    config: RwLock<StorageConfig>,
    /// Statistics
    stats: RwLock<StorageStats>,
    /// ID generators
    next_pool_id: AtomicU64,
    next_disk_id: AtomicU64,
    next_snapshot_id: AtomicU64,
}

/// Storage pool identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StoragePoolId(u64);

impl StoragePoolId {
    pub fn new(id: u64) -> Self { Self(id) }
    pub fn as_u64(&self) -> u64 { self.0 }
}

/// Virtual disk identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VirtualDiskId(u64);

impl VirtualDiskId {
    pub fn new(id: u64) -> Self { Self(id) }
    pub fn as_u64(&self) -> u64 { self.0 }
}

/// Snapshot identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SnapshotId(u64);

impl SnapshotId {
    pub fn new(id: u64) -> Self { Self(id) }
    pub fn as_u64(&self) -> u64 { self.0 }
}

/// Storage configuration
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// Enable thin provisioning by default
    pub default_thin_provisioning: bool,
    /// Enable deduplication
    pub deduplication: bool,
    /// Enable compression
    pub compression: bool,
    /// Compression algorithm
    pub compression_algo: CompressionAlgorithm,
    /// Default disk format
    pub default_format: DiskFormat,
    /// Maximum snapshot chain length
    pub max_snapshot_depth: u32,
    /// Enable storage cache
    pub enable_cache: bool,
    /// Cache size (MB)
    pub cache_size_mb: u64,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            default_thin_provisioning: true,
            deduplication: true,
            compression: true,
            compression_algo: CompressionAlgorithm::Zstd,
            default_format: DiskFormat::Qcow2,
            max_snapshot_depth: 32,
            enable_cache: true,
            cache_size_mb: 1024,
        }
    }
}

/// Compression algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionAlgorithm {
    None,
    Gzip,
    Lz4,
    Zstd,
}

/// Storage statistics
#[derive(Debug, Clone, Default)]
pub struct StorageStats {
    pub total_capacity: u64,
    pub used_capacity: u64,
    pub allocated_capacity: u64,
    pub dedup_savings: u64,
    pub compression_savings: u64,
    pub read_ops: u64,
    pub write_ops: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
}

impl StorageManager {
    pub fn new() -> Self {
        Self {
            pools: RwLock::new(HashMap::new()),
            disks: RwLock::new(HashMap::new()),
            snapshots: RwLock::new(HashMap::new()),
            backends: RwLock::new(HashMap::new()),
            config: RwLock::new(StorageConfig::default()),
            stats: RwLock::new(StorageStats::default()),
            next_pool_id: AtomicU64::new(1),
            next_disk_id: AtomicU64::new(1),
            next_snapshot_id: AtomicU64::new(1),
        }
    }
    
    /// Configure storage
    pub fn configure(&self, config: StorageConfig) {
        *self.config.write().unwrap() = config;
    }
    
    /// Create storage pool
    pub fn create_pool(&self, name: &str, spec: StoragePoolSpec) -> HypervisorResult<StoragePoolId> {
        let id = StoragePoolId::new(self.next_pool_id.fetch_add(1, Ordering::SeqCst));
        
        let pool = Arc::new(StoragePool::new(id, name.to_string(), spec));
        self.pools.write().unwrap().insert(id, pool);
        
        Ok(id)
    }
    
    /// Delete storage pool
    pub fn delete_pool(&self, id: StoragePoolId) -> HypervisorResult<()> {
        let pool = self.get_pool(id)?;
        
        // Check if pool has disks
        if pool.disk_count() > 0 {
            return Err(HypervisorError::StorageError(
                "Cannot delete pool with existing disks".to_string()
            ));
        }
        
        self.pools.write().unwrap().remove(&id);
        Ok(())
    }
    
    /// Create virtual disk
    pub fn create_disk(&self, spec: VirtualDiskSpec) -> HypervisorResult<VirtualDiskId> {
        let id = VirtualDiskId::new(self.next_disk_id.fetch_add(1, Ordering::SeqCst));
        let config = self.config.read().unwrap();
        
        let disk = Arc::new(VirtualDisk::new(
            id,
            spec,
            config.default_thin_provisioning,
        ));
        
        // Add to pool if specified
        if let Some(pool_id) = disk.pool_id {
            let pool = self.get_pool(pool_id)?;
            pool.add_disk(id, disk.allocated_size())?;
        }
        
        self.disks.write().unwrap().insert(id, disk);
        
        Ok(id)
    }
    
    /// Delete virtual disk
    pub fn delete_disk(&self, id: VirtualDiskId) -> HypervisorResult<()> {
        let disk = self.get_disk(id)?;
        
        // Check if disk is attached
        if disk.attached_to.read().unwrap().is_some() {
            return Err(HypervisorError::StorageError(
                "Cannot delete attached disk".to_string()
            ));
        }
        
        // Check for dependent snapshots
        if disk.snapshot_count() > 0 {
            return Err(HypervisorError::StorageError(
                "Cannot delete disk with snapshots".to_string()
            ));
        }
        
        // Remove from pool
        if let Some(pool_id) = disk.pool_id {
            if let Ok(pool) = self.get_pool(pool_id) {
                pool.remove_disk(id);
            }
        }
        
        self.disks.write().unwrap().remove(&id);
        Ok(())
    }
    
    /// Resize virtual disk
    pub fn resize_disk(&self, id: VirtualDiskId, new_size: u64) -> HypervisorResult<()> {
        let disk = self.get_disk(id)?;
        
        if new_size < disk.size() {
            return Err(HypervisorError::StorageError(
                "Cannot shrink disk".to_string()
            ));
        }
        
        disk.resize(new_size)?;
        Ok(())
    }
    
    /// Attach disk to VM
    pub fn attach_disk(&self, disk_id: VirtualDiskId, vm_id: VmId) -> HypervisorResult<()> {
        let disk = self.get_disk(disk_id)?;
        
        if disk.attached_to.read().unwrap().is_some() {
            return Err(HypervisorError::StorageError(
                "Disk already attached".to_string()
            ));
        }
        
        *disk.attached_to.write().unwrap() = Some(vm_id);
        Ok(())
    }
    
    /// Detach disk from VM
    pub fn detach_disk(&self, disk_id: VirtualDiskId) -> HypervisorResult<()> {
        let disk = self.get_disk(disk_id)?;
        *disk.attached_to.write().unwrap() = None;
        Ok(())
    }
    
    /// Create snapshot
    pub fn create_snapshot(&self, disk_id: VirtualDiskId, name: &str) -> HypervisorResult<SnapshotId> {
        let disk = self.get_disk(disk_id)?;
        let config = self.config.read().unwrap();
        
        // Check snapshot depth
        let depth = disk.snapshot_depth();
        if depth >= config.max_snapshot_depth {
            return Err(HypervisorError::StorageError(
                format!("Maximum snapshot depth ({}) exceeded", config.max_snapshot_depth)
            ));
        }
        
        let snapshot_id = SnapshotId::new(self.next_snapshot_id.fetch_add(1, Ordering::SeqCst));
        
        let snapshot = Arc::new(Snapshot::new(
            snapshot_id,
            disk_id,
            name.to_string(),
            disk.current_snapshot.read().unwrap().clone(),
        ));
        
        // Update disk's current snapshot
        *disk.current_snapshot.write().unwrap() = Some(snapshot_id);
        disk.add_snapshot(snapshot_id);
        
        self.snapshots.write().unwrap().insert(snapshot_id, snapshot);
        
        Ok(snapshot_id)
    }
    
    /// Delete snapshot
    pub fn delete_snapshot(&self, id: SnapshotId) -> HypervisorResult<()> {
        let snapshot = self.get_snapshot(id)?;
        
        // Check if snapshot has children
        if snapshot.child_count() > 0 {
            return Err(HypervisorError::StorageError(
                "Cannot delete snapshot with children".to_string()
            ));
        }
        
        // Remove from parent
        if let Some(parent_id) = snapshot.parent {
            if let Ok(parent) = self.get_snapshot(parent_id) {
                parent.remove_child(id);
            }
        }
        
        // Remove from disk
        let disk = self.get_disk(snapshot.disk_id)?;
        disk.remove_snapshot(id);
        
        self.snapshots.write().unwrap().remove(&id);
        Ok(())
    }
    
    /// Revert to snapshot
    pub fn revert_to_snapshot(&self, id: SnapshotId) -> HypervisorResult<()> {
        let snapshot = self.get_snapshot(id)?;
        let disk = self.get_disk(snapshot.disk_id)?;
        
        // Check if disk is attached
        if disk.attached_to.read().unwrap().is_some() {
            return Err(HypervisorError::StorageError(
                "Cannot revert attached disk".to_string()
            ));
        }
        
        // Update current snapshot
        *disk.current_snapshot.write().unwrap() = Some(id);
        
        Ok(())
    }
    
    /// Clone disk (linked clone)
    pub fn clone_disk_linked(&self, source_id: VirtualDiskId, name: &str) -> HypervisorResult<VirtualDiskId> {
        let source = self.get_disk(source_id)?;
        
        // Create snapshot as base
        let snapshot_id = self.create_snapshot(source_id, &format!("{}-base", name))?;
        
        // Create new disk based on snapshot
        let spec = VirtualDiskSpec {
            name: name.to_string(),
            size: source.size(),
            format: source.format,
            pool_id: source.pool_id,
            backing_disk: Some(source_id),
            backing_snapshot: Some(snapshot_id),
        };
        
        self.create_disk(spec)
    }
    
    /// Clone disk (full clone)
    pub fn clone_disk_full(&self, source_id: VirtualDiskId, name: &str) -> HypervisorResult<VirtualDiskId> {
        let source = self.get_disk(source_id)?;
        
        let spec = VirtualDiskSpec {
            name: name.to_string(),
            size: source.size(),
            format: source.format,
            pool_id: source.pool_id,
            backing_disk: None,
            backing_snapshot: None,
        };
        
        let new_id = self.create_disk(spec)?;
        
        // Copy data (simulated)
        // In real implementation, this would copy block by block
        
        Ok(new_id)
    }
    
    /// Migrate disk to different pool
    pub fn migrate_disk(&self, disk_id: VirtualDiskId, target_pool: StoragePoolId) -> HypervisorResult<()> {
        let disk = self.get_disk(disk_id)?;
        let target = self.get_pool(target_pool)?;
        
        // Check space
        if target.available() < disk.allocated_size() {
            return Err(HypervisorError::StorageError(
                "Insufficient space in target pool".to_string()
            ));
        }
        
        // Remove from source pool
        if let Some(source_pool_id) = disk.pool_id {
            if let Ok(source) = self.get_pool(source_pool_id) {
                source.remove_disk(disk_id);
            }
        }
        
        // Add to target pool
        target.add_disk(disk_id, disk.allocated_size())?;
        
        Ok(())
    }
    
    /// Get storage statistics
    pub fn stats(&self) -> StorageStats {
        self.stats.read().unwrap().clone()
    }
    
    /// List all pools
    pub fn list_pools(&self) -> Vec<StoragePoolId> {
        self.pools.read().unwrap().keys().copied().collect()
    }
    
    /// List all disks
    pub fn list_disks(&self) -> Vec<VirtualDiskId> {
        self.disks.read().unwrap().keys().copied().collect()
    }
    
    fn get_pool(&self, id: StoragePoolId) -> HypervisorResult<Arc<StoragePool>> {
        self.pools.read().unwrap()
            .get(&id)
            .cloned()
            .ok_or(HypervisorError::StorageError(format!("Pool {} not found", id.0)))
    }
    
    fn get_disk(&self, id: VirtualDiskId) -> HypervisorResult<Arc<VirtualDisk>> {
        self.disks.read().unwrap()
            .get(&id)
            .cloned()
            .ok_or(HypervisorError::StorageError(format!("Disk {} not found", id.0)))
    }
    
    fn get_snapshot(&self, id: SnapshotId) -> HypervisorResult<Arc<Snapshot>> {
        self.snapshots.read().unwrap()
            .get(&id)
            .cloned()
            .ok_or(HypervisorError::StorageError(format!("Snapshot {} not found", id.0)))
    }
}

impl Default for StorageManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Storage Pool
// ============================================================================

/// Storage pool
pub struct StoragePool {
    id: StoragePoolId,
    name: String,
    spec: StoragePoolSpec,
    disks: RwLock<HashMap<VirtualDiskId, u64>>, // disk -> allocated size
    used: AtomicU64,
    stats: RwLock<PoolStats>,
}

/// Storage pool specification
#[derive(Debug, Clone)]
pub struct StoragePoolSpec {
    /// Pool type
    pub pool_type: StoragePoolType,
    /// Total capacity (bytes)
    pub capacity: u64,
    /// Storage path
    pub path: PathBuf,
    /// Enable overcommit
    pub overcommit: bool,
    /// Overcommit ratio (e.g., 2.0 = 200%)
    pub overcommit_ratio: f64,
    /// Storage tier
    pub tier: StorageTier,
}

impl Default for StoragePoolSpec {
    fn default() -> Self {
        Self {
            pool_type: StoragePoolType::Local,
            capacity: 100 * 1024 * 1024 * 1024, // 100GB
            path: PathBuf::from("/var/lib/hypervisor/storage"),
            overcommit: true,
            overcommit_ratio: 2.0,
            tier: StorageTier::Standard,
        }
    }
}

/// Storage pool type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoragePoolType {
    Local,
    Nfs,
    Iscsi,
    Ceph,
    Gluster,
    S3,
}

/// Storage tier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageTier {
    /// NVMe/SSD - highest performance
    Premium,
    /// SSD
    Fast,
    /// HDD
    Standard,
    /// Archive storage
    Archive,
}

/// Pool statistics
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    pub read_ops: u64,
    pub write_ops: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub iops: u64,
    pub latency_ms: f64,
}

impl StoragePool {
    pub fn new(id: StoragePoolId, name: String, spec: StoragePoolSpec) -> Self {
        Self {
            id,
            name,
            spec,
            disks: RwLock::new(HashMap::new()),
            used: AtomicU64::new(0),
            stats: RwLock::new(PoolStats::default()),
        }
    }
    
    pub fn id(&self) -> StoragePoolId { self.id }
    pub fn name(&self) -> &str { &self.name }
    pub fn capacity(&self) -> u64 { self.spec.capacity }
    
    pub fn used(&self) -> u64 { 
        self.used.load(Ordering::SeqCst) 
    }
    
    pub fn available(&self) -> u64 {
        let capacity = if self.spec.overcommit {
            (self.spec.capacity as f64 * self.spec.overcommit_ratio) as u64
        } else {
            self.spec.capacity
        };
        capacity.saturating_sub(self.used())
    }
    
    pub fn disk_count(&self) -> usize {
        self.disks.read().unwrap().len()
    }
    
    pub fn add_disk(&self, id: VirtualDiskId, size: u64) -> HypervisorResult<()> {
        if self.available() < size {
            return Err(HypervisorError::StorageError(
                "Insufficient pool space".to_string()
            ));
        }
        
        self.disks.write().unwrap().insert(id, size);
        self.used.fetch_add(size, Ordering::SeqCst);
        Ok(())
    }
    
    pub fn remove_disk(&self, id: VirtualDiskId) {
        if let Some(size) = self.disks.write().unwrap().remove(&id) {
            self.used.fetch_sub(size, Ordering::SeqCst);
        }
    }
    
    pub fn stats(&self) -> PoolStats {
        self.stats.read().unwrap().clone()
    }
}

// ============================================================================
// Virtual Disk
// ============================================================================

/// Virtual disk
pub struct VirtualDisk {
    id: VirtualDiskId,
    name: String,
    format: DiskFormat,
    size: AtomicU64,
    allocated: AtomicU64,
    pool_id: Option<StoragePoolId>,
    attached_to: RwLock<Option<VmId>>,
    backing_disk: Option<VirtualDiskId>,
    backing_snapshot: Option<SnapshotId>,
    current_snapshot: RwLock<Option<SnapshotId>>,
    snapshots: RwLock<Vec<SnapshotId>>,
    thin_provisioned: bool,
    created_at: Instant,
    stats: RwLock<DiskStats>,
}

/// Virtual disk specification
#[derive(Debug, Clone)]
pub struct VirtualDiskSpec {
    /// Disk name
    pub name: String,
    /// Virtual size (bytes)
    pub size: u64,
    /// Disk format
    pub format: DiskFormat,
    /// Storage pool
    pub pool_id: Option<StoragePoolId>,
    /// Backing disk (for linked clones)
    pub backing_disk: Option<VirtualDiskId>,
    /// Backing snapshot
    pub backing_snapshot: Option<SnapshotId>,
}

impl Default for VirtualDiskSpec {
    fn default() -> Self {
        Self {
            name: "disk".to_string(),
            size: 10 * 1024 * 1024 * 1024, // 10GB
            format: DiskFormat::Qcow2,
            pool_id: None,
            backing_disk: None,
            backing_snapshot: None,
        }
    }
}

/// Disk format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskFormat {
    /// RAW format
    Raw,
    /// QEMU Copy-on-Write v2
    Qcow2,
    /// VMware VMDK
    Vmdk,
    /// VirtualBox VDI
    Vdi,
    /// Virtual Hard Disk (Hyper-V)
    Vhd,
    /// Virtual Hard Disk X (Hyper-V)
    Vhdx,
}

impl DiskFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Raw => "img",
            Self::Qcow2 => "qcow2",
            Self::Vmdk => "vmdk",
            Self::Vdi => "vdi",
            Self::Vhd => "vhd",
            Self::Vhdx => "vhdx",
        }
    }
    
    pub fn supports_snapshots(&self) -> bool {
        matches!(self, Self::Qcow2 | Self::Vmdk | Self::Vdi | Self::Vhdx)
    }
    
    pub fn supports_thin_provisioning(&self) -> bool {
        matches!(self, Self::Qcow2 | Self::Vmdk | Self::Vdi | Self::Vhdx)
    }
}

/// Disk statistics
#[derive(Debug, Clone, Default)]
pub struct DiskStats {
    pub read_ops: u64,
    pub write_ops: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub flush_ops: u64,
}

impl VirtualDisk {
    pub fn new(id: VirtualDiskId, spec: VirtualDiskSpec, thin: bool) -> Self {
        let initial_allocated = if thin { 0 } else { spec.size };
        
        Self {
            id,
            name: spec.name,
            format: spec.format,
            size: AtomicU64::new(spec.size),
            allocated: AtomicU64::new(initial_allocated),
            pool_id: spec.pool_id,
            attached_to: RwLock::new(None),
            backing_disk: spec.backing_disk,
            backing_snapshot: spec.backing_snapshot,
            current_snapshot: RwLock::new(None),
            snapshots: RwLock::new(Vec::new()),
            thin_provisioned: thin,
            created_at: Instant::now(),
            stats: RwLock::new(DiskStats::default()),
        }
    }
    
    pub fn id(&self) -> VirtualDiskId { self.id }
    pub fn name(&self) -> &str { &self.name }
    pub fn format(&self) -> DiskFormat { self.format }
    pub fn size(&self) -> u64 { self.size.load(Ordering::SeqCst) }
    pub fn allocated_size(&self) -> u64 { self.allocated.load(Ordering::SeqCst) }
    pub fn is_thin(&self) -> bool { self.thin_provisioned }
    pub fn is_linked_clone(&self) -> bool { self.backing_disk.is_some() }
    
    pub fn resize(&self, new_size: u64) -> HypervisorResult<()> {
        if new_size < self.size() {
            return Err(HypervisorError::StorageError(
                "Cannot shrink disk".to_string()
            ));
        }
        self.size.store(new_size, Ordering::SeqCst);
        Ok(())
    }
    
    pub fn snapshot_count(&self) -> usize {
        self.snapshots.read().unwrap().len()
    }
    
    pub fn snapshot_depth(&self) -> u32 {
        // Count chain depth
        let mut depth = 0u32;
        let mut current = *self.current_snapshot.read().unwrap();
        
        // Simplified - would need snapshot access in real impl
        while current.is_some() {
            depth += 1;
            current = None; // Would traverse parent chain
        }
        
        depth
    }
    
    pub fn add_snapshot(&self, id: SnapshotId) {
        self.snapshots.write().unwrap().push(id);
    }
    
    pub fn remove_snapshot(&self, id: SnapshotId) {
        self.snapshots.write().unwrap().retain(|&s| s != id);
    }
    
    /// Read from disk (simulated)
    pub fn read(&self, offset: u64, size: usize) -> HypervisorResult<Vec<u8>> {
        if offset + size as u64 > self.size() {
            return Err(HypervisorError::StorageError("Read beyond disk size".to_string()));
        }
        
        let mut stats = self.stats.write().unwrap();
        stats.read_ops += 1;
        stats.read_bytes += size as u64;
        
        // Return zeros for testing
        Ok(vec![0u8; size])
    }
    
    /// Write to disk (simulated)
    pub fn write(&self, offset: u64, data: &[u8]) -> HypervisorResult<()> {
        if offset + data.len() as u64 > self.size() {
            return Err(HypervisorError::StorageError("Write beyond disk size".to_string()));
        }
        
        let mut stats = self.stats.write().unwrap();
        stats.write_ops += 1;
        stats.write_bytes += data.len() as u64;
        
        // Update allocated size for thin provisioning
        if self.thin_provisioned {
            let end = offset + data.len() as u64;
            let current = self.allocated.load(Ordering::SeqCst);
            if end > current {
                self.allocated.store(end, Ordering::SeqCst);
            }
        }
        
        Ok(())
    }
    
    /// Flush to disk
    pub fn flush(&self) -> HypervisorResult<()> {
        self.stats.write().unwrap().flush_ops += 1;
        Ok(())
    }
    
    pub fn stats(&self) -> DiskStats {
        self.stats.read().unwrap().clone()
    }
}

// ============================================================================
// Snapshot
// ============================================================================

/// Disk snapshot
pub struct Snapshot {
    id: SnapshotId,
    disk_id: VirtualDiskId,
    name: String,
    parent: Option<SnapshotId>,
    children: RwLock<Vec<SnapshotId>>,
    created_at: Instant,
    size: u64,
}

impl Snapshot {
    pub fn new(
        id: SnapshotId,
        disk_id: VirtualDiskId,
        name: String,
        parent: Option<SnapshotId>,
    ) -> Self {
        Self {
            id,
            disk_id,
            name,
            parent,
            children: RwLock::new(Vec::new()),
            created_at: Instant::now(),
            size: 0,
        }
    }
    
    pub fn id(&self) -> SnapshotId { self.id }
    pub fn disk_id(&self) -> VirtualDiskId { self.disk_id }
    pub fn name(&self) -> &str { &self.name }
    pub fn parent(&self) -> Option<SnapshotId> { self.parent }
    pub fn created_at(&self) -> Instant { self.created_at }
    
    pub fn child_count(&self) -> usize {
        self.children.read().unwrap().len()
    }
    
    pub fn add_child(&self, id: SnapshotId) {
        self.children.write().unwrap().push(id);
    }
    
    pub fn remove_child(&self, id: SnapshotId) {
        self.children.write().unwrap().retain(|&c| c != id);
    }
}

// ============================================================================
// Storage Backend
// ============================================================================

/// Storage backend trait
pub trait StorageBackend: Send + Sync {
    /// Backend name
    fn name(&self) -> &str;
    
    /// Initialize backend
    fn init(&self) -> HypervisorResult<()>;
    
    /// Create disk image
    fn create_image(&self, path: &str, size: u64, format: DiskFormat) -> HypervisorResult<()>;
    
    /// Delete disk image
    fn delete_image(&self, path: &str) -> HypervisorResult<()>;
    
    /// Read from image
    fn read(&self, path: &str, offset: u64, size: usize) -> HypervisorResult<Vec<u8>>;
    
    /// Write to image
    fn write(&self, path: &str, offset: u64, data: &[u8]) -> HypervisorResult<()>;
    
    /// Flush writes
    fn flush(&self, path: &str) -> HypervisorResult<()>;
    
    /// Get image info
    fn info(&self, path: &str) -> HypervisorResult<ImageInfo>;
}

/// Image information
#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub format: DiskFormat,
    pub virtual_size: u64,
    pub actual_size: u64,
    pub has_backing: bool,
    pub backing_file: Option<String>,
}

/// Local file backend
pub struct LocalStorageBackend {
    name: String,
}

impl LocalStorageBackend {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string() }
    }
}

impl StorageBackend for LocalStorageBackend {
    fn name(&self) -> &str { &self.name }
    
    fn init(&self) -> HypervisorResult<()> { Ok(()) }
    
    fn create_image(&self, _path: &str, _size: u64, _format: DiskFormat) -> HypervisorResult<()> {
        // Simulated - would create actual file
        Ok(())
    }
    
    fn delete_image(&self, _path: &str) -> HypervisorResult<()> {
        Ok(())
    }
    
    fn read(&self, _path: &str, _offset: u64, size: usize) -> HypervisorResult<Vec<u8>> {
        Ok(vec![0u8; size])
    }
    
    fn write(&self, _path: &str, _offset: u64, _data: &[u8]) -> HypervisorResult<()> {
        Ok(())
    }
    
    fn flush(&self, _path: &str) -> HypervisorResult<()> {
        Ok(())
    }
    
    fn info(&self, _path: &str) -> HypervisorResult<ImageInfo> {
        Ok(ImageInfo {
            format: DiskFormat::Qcow2,
            virtual_size: 0,
            actual_size: 0,
            has_backing: false,
            backing_file: None,
        })
    }
}

// ============================================================================
// Storage Cache
// ============================================================================

/// Storage cache for read acceleration
pub struct StorageCache {
    /// Cache entries (block hash -> data)
    entries: RwLock<HashMap<u64, CacheEntry>>,
    /// Maximum cache size
    max_size: u64,
    /// Current cache size
    current_size: AtomicU64,
    /// Statistics
    stats: RwLock<CacheStats>,
}

struct CacheEntry {
    data: Vec<u8>,
    last_access: Instant,
    hit_count: u64,
}

#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub size: u64,
}

impl StorageCache {
    pub fn new(max_size_mb: u64) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            max_size: max_size_mb * 1024 * 1024,
            current_size: AtomicU64::new(0),
            stats: RwLock::new(CacheStats::default()),
        }
    }
    
    /// Get from cache
    pub fn get(&self, key: u64) -> Option<Vec<u8>> {
        let mut entries = self.entries.write().unwrap();
        
        if let Some(entry) = entries.get_mut(&key) {
            entry.last_access = Instant::now();
            entry.hit_count += 1;
            self.stats.write().unwrap().hits += 1;
            return Some(entry.data.clone());
        }
        
        self.stats.write().unwrap().misses += 1;
        None
    }
    
    /// Put into cache
    pub fn put(&self, key: u64, data: Vec<u8>) {
        let size = data.len() as u64;
        
        // Evict if needed
        while self.current_size.load(Ordering::SeqCst) + size > self.max_size {
            if !self.evict_one() {
                break;
            }
        }
        
        let entry = CacheEntry {
            data,
            last_access: Instant::now(),
            hit_count: 0,
        };
        
        self.entries.write().unwrap().insert(key, entry);
        self.current_size.fetch_add(size, Ordering::SeqCst);
        self.stats.write().unwrap().size = self.current_size.load(Ordering::SeqCst);
    }
    
    /// Evict one entry (LRU)
    fn evict_one(&self) -> bool {
        let mut entries = self.entries.write().unwrap();
        
        if let Some((&key, entry)) = entries.iter()
            .min_by_key(|(_, e)| e.last_access)
        {
            let size = entry.data.len() as u64;
            entries.remove(&key);
            self.current_size.fetch_sub(size, Ordering::SeqCst);
            self.stats.write().unwrap().evictions += 1;
            return true;
        }
        
        false
    }
    
    /// Clear cache
    pub fn clear(&self) {
        self.entries.write().unwrap().clear();
        self.current_size.store(0, Ordering::SeqCst);
    }
    
    pub fn stats(&self) -> CacheStats {
        self.stats.read().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_storage_manager() {
        let manager = StorageManager::new();
        
        // Create pool
        let pool_id = manager.create_pool("default", StoragePoolSpec::default()).unwrap();
        
        // Create disk
        let disk_spec = VirtualDiskSpec {
            name: "test-disk".to_string(),
            size: 10 * 1024 * 1024 * 1024,
            format: DiskFormat::Qcow2,
            pool_id: Some(pool_id),
            ..Default::default()
        };
        
        let disk_id = manager.create_disk(disk_spec).unwrap();
        
        // Create snapshot
        let snap_id = manager.create_snapshot(disk_id, "snap1").unwrap();
        
        // Revert not possible when attached
        manager.attach_disk(disk_id, VmId::new(1)).unwrap();
        assert!(manager.revert_to_snapshot(snap_id).is_err());
        
        manager.detach_disk(disk_id).unwrap();
        manager.revert_to_snapshot(snap_id).unwrap();
    }
    
    #[test]
    fn test_disk_operations() {
        let spec = VirtualDiskSpec {
            name: "test".to_string(),
            size: 1024 * 1024,
            format: DiskFormat::Qcow2,
            ..Default::default()
        };
        
        let disk = VirtualDisk::new(VirtualDiskId::new(1), spec, true);
        
        // Write data
        disk.write(0, &[1, 2, 3, 4]).unwrap();
        disk.write(1000, &[5, 6, 7, 8]).unwrap();
        
        // Read data
        let data = disk.read(0, 4).unwrap();
        assert_eq!(data.len(), 4);
        
        // Check stats
        let stats = disk.stats();
        assert_eq!(stats.write_ops, 2);
        assert_eq!(stats.read_ops, 1);
    }
    
    #[test]
    fn test_storage_cache() {
        let cache = StorageCache::new(1); // 1MB
        
        // Put data
        cache.put(1, vec![0u8; 1024]);
        cache.put(2, vec![0u8; 1024]);
        
        // Get data
        assert!(cache.get(1).is_some());
        assert!(cache.get(2).is_some());
        assert!(cache.get(3).is_none());
        
        let stats = cache.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
    }
    
    #[test]
    fn test_disk_formats() {
        assert!(DiskFormat::Qcow2.supports_snapshots());
        assert!(DiskFormat::Qcow2.supports_thin_provisioning());
        assert!(!DiskFormat::Raw.supports_snapshots());
        assert_eq!(DiskFormat::Vmdk.extension(), "vmdk");
    }
}
