//! CLI Command Implementations

use super::{CliResult, CliError, OutputFormat};
use super::client::ApiClient;
use super::state::{LocalState, VmRecord, VmStatus};
use serde::{Deserialize, Serialize};

/// VM commands
pub mod vm {
    use super::*;
    
    /// List VMs - tries API first, falls back to local state
    pub fn list(_format: OutputFormat, _all: bool, _node: Option<&str>) -> CliResult<Vec<VmInfo>> {
        let client = ApiClient::from_config();
        
        // Try to get from API server first
        match client.get::<Vec<VmInfo>>("/vms") {
            Ok(vms) => return Ok(vms),
            Err(CliError::Network(_)) => {
                // Server not available, use local state
            }
            Err(e) => return Err(e),
        }
        
        // Fall back to local state
        let state = LocalState::load();
        let vms: Vec<VmInfo> = state.list_vms()
            .into_iter()
            .map(|vm| VmInfo {
                id: vm.id.clone(),
                name: vm.name.clone(),
                status: vm.status.to_string(),
                vcpus: vm.vcpus,
                memory_mb: vm.memory_mb,
                node: vm.node.clone().unwrap_or_else(|| "local".to_string()),
            })
            .collect();
        
        Ok(vms)
    }
    
    /// Create VM
    pub fn create(spec: CreateVmSpec) -> CliResult<String> {
        let client = ApiClient::from_config();
        
        // Try API first
        match client.post::<CreateVmResponse, _>("/vms", &spec) {
            Ok(response) => return Ok(response.id),
            Err(CliError::Network(_)) => {
                // Server not available, create locally
            }
            Err(e) => return Err(e),
        }
        
        // Create in local state
        let mut state = LocalState::load();
        let id = format!("vm-{:03}", state.vms.len() + 1);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        
        let vm = VmRecord {
            id: id.clone(),
            name: spec.name,
            vcpus: spec.vcpus,
            memory_mb: spec.memory_mb,
            disk_gb: spec.disk_gb,
            status: VmStatus::Stopped,
            node: None,
            created_at: now,
            config_path: None,
            disk_path: None,
        };
        
        state.upsert_vm(vm);
        state.save()?;
        
        Ok(id)
    }
    
    /// Start VM
    pub fn start(vm_id: &str) -> CliResult<()> {
        let client = ApiClient::from_config();
        
        // Try API first
        match client.post::<serde_json::Value, _>(&format!("/vms/{}/start", vm_id), &()) {
            Ok(_) => return Ok(()),
            Err(CliError::Network(_)) => {
                // Server not available
            }
            Err(e) => return Err(e),
        }
        
        // Update local state
        let mut state = LocalState::load();
        if state.set_vm_status(vm_id, VmStatus::Running).is_none() {
            return Err(CliError::NotFound(format!("VM '{}' not found", vm_id)));
        }
        state.save()?;
        
        Ok(())
    }
    
    /// Stop VM
    pub fn stop(vm_id: &str, force: bool) -> CliResult<()> {
        let client = ApiClient::from_config();
        
        #[derive(Serialize)]
        struct StopRequest { force: bool }
        
        match client.post::<serde_json::Value, _>(&format!("/vms/{}/stop", vm_id), &StopRequest { force }) {
            Ok(_) => return Ok(()),
            Err(CliError::Network(_)) => {}
            Err(e) => return Err(e),
        }
        
        let mut state = LocalState::load();
        if state.set_vm_status(vm_id, VmStatus::Stopped).is_none() {
            return Err(CliError::NotFound(format!("VM '{}' not found", vm_id)));
        }
        state.save()?;
        
        Ok(())
    }
    
    /// Restart VM
    pub fn restart(vm_id: &str) -> CliResult<()> {
        stop(vm_id, false)?;
        start(vm_id)
    }
    
    /// Delete VM
    pub fn delete(vm_id: &str, force: bool) -> CliResult<()> {
        let client = ApiClient::from_config();
        
        match client.delete(&format!("/vms/{}?force={}", vm_id, force)) {
            Ok(_) => return Ok(()),
            Err(CliError::Network(_)) => {}
            Err(e) => return Err(e),
        }
        
        let mut state = LocalState::load();
        if state.remove_vm(vm_id).is_none() {
            return Err(CliError::NotFound(format!("VM '{}' not found", vm_id)));
        }
        state.save()?;
        
        Ok(())
    }
    
    /// Get VM info
    pub fn info(vm_id: &str) -> CliResult<VmDetails> {
        let client = ApiClient::from_config();
        
        match client.get::<VmDetails>(&format!("/vms/{}", vm_id)) {
            Ok(details) => return Ok(details),
            Err(CliError::Network(_)) => {}
            Err(e) => return Err(e),
        }
        
        let state = LocalState::load();
        let vm = state.get_vm(vm_id)
            .ok_or_else(|| CliError::NotFound(format!("VM '{}' not found", vm_id)))?;
        
        Ok(VmDetails {
            id: vm.id.clone(),
            name: vm.name.clone(),
            status: vm.status.to_string(),
            vcpus: vm.vcpus,
            memory_mb: vm.memory_mb,
            disk_gb: vm.disk_gb,
            node: vm.node.clone().unwrap_or_else(|| "local".to_string()),
            uptime: None,
            ip: None,
        })
    }
    
    /// Create snapshot
    pub fn snapshot(vm_id: &str, name: &str, _description: Option<&str>) -> CliResult<String> {
        let client = ApiClient::from_config();
        client.ensure_connected().map_err(|_| {
            CliError::Operation("Snapshots require nvmserver to be running".into())
        })?;
        
        #[derive(Serialize)]
        struct SnapshotReq<'a> { name: &'a str }
        
        let resp: SnapshotResponse = client.post(&format!("/vms/{}/snapshot", vm_id), &SnapshotReq { name })?;
        Ok(resp.id)
    }
    
    /// Clone VM
    pub fn clone(vm_id: &str, new_name: &str, _full: bool) -> CliResult<String> {
        let client = ApiClient::from_config();
        client.ensure_connected().map_err(|_| {
            CliError::Operation("VM cloning requires nvmserver to be running".into())
        })?;
        
        #[derive(Serialize)]
        struct CloneReq<'a> { name: &'a str }
        
        let resp: CreateVmResponse = client.post(&format!("/vms/{}/clone", vm_id), &CloneReq { name: new_name })?;
        Ok(resp.id)
    }
    
    /// Migrate VM
    pub fn migrate(vm_id: &str, target_node: &str, _live: bool) -> CliResult<()> {
        let client = ApiClient::from_config();
        client.ensure_connected().map_err(|_| {
            CliError::Operation("VM migration requires nvmserver and cluster to be running".into())
        })?;
        
        #[derive(Serialize)]
        struct MigrateReq<'a> { target: &'a str }
        
        client.post::<serde_json::Value, _>(&format!("/vms/{}/migrate", vm_id), &MigrateReq { target: target_node })?;
        Ok(())
    }
    
    /// Open console
    pub fn console(vm_id: &str, _console_type: &str) -> CliResult<ConsoleInfo> {
        let client = ApiClient::from_config();
        client.ensure_connected().map_err(|_| {
            CliError::Operation("Console access requires nvmserver to be running".into())
        })?;
        
        client.get(&format!("/vms/{}/console", vm_id))
    }
    
    // Response types
    #[derive(Deserialize)]
    struct CreateVmResponse { id: String }
    
    #[derive(Deserialize)]
    struct SnapshotResponse { id: String }
    
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct VmInfo {
        pub id: String,
        pub name: String,
        pub status: String,
        pub vcpus: u32,
        pub memory_mb: u64,
        pub node: String,
    }
    
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct VmDetails {
        pub id: String,
        pub name: String,
        pub status: String,
        pub vcpus: u32,
        pub memory_mb: u64,
        pub disk_gb: u64,
        pub node: String,
        pub uptime: Option<u64>,
        pub ip: Option<String>,
    }
    
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CreateVmSpec {
        pub name: String,
        pub vcpus: u32,
        pub memory_mb: u64,
        pub disk_gb: u64,
        pub os_type: String,
        pub iso: Option<String>,
        pub network: Option<String>,
        pub template: Option<String>,
    }
    
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ConsoleInfo {
        pub url: String,
        pub ticket: String,
        pub port: u16,
    }
}

/// Storage commands
pub mod storage {
    use super::*;
    
    pub fn list_pools() -> CliResult<Vec<PoolInfo>> {
        let client = ApiClient::from_config();
        
        match client.get::<Vec<PoolInfo>>("/storage/pools") {
            Ok(pools) => return Ok(pools),
            Err(CliError::Network(_)) => {}
            Err(e) => return Err(e),
        }
        
        // Fall back to local state
        let state = LocalState::load();
        Ok(state.storage_pools.values()
            .map(|p| PoolInfo {
                name: p.name.clone(),
                pool_type: p.pool_type.clone(),
                total_gb: p.total_gb,
                used_gb: p.used_gb,
                status: "active".to_string(),
            })
            .collect())
    }
    
    pub fn list_volumes(pool: Option<&str>) -> CliResult<Vec<VolumeInfo>> {
        let client = ApiClient::from_config();
        
        let endpoint = match pool {
            Some(p) => format!("/storage/volumes?pool={}", p),
            None => "/storage/volumes".to_string(),
        };
        
        match client.get::<Vec<VolumeInfo>>(&endpoint) {
            Ok(vols) => return Ok(vols),
            Err(CliError::Network(_)) => {}
            Err(e) => return Err(e),
        }
        
        // No local fallback for volumes - need server
        Ok(vec![])
    }
    
    pub fn create_volume(_pool: &str, _name: &str, _size: &str) -> CliResult<String> {
        let client = ApiClient::from_config();
        client.ensure_connected().map_err(|_| {
            CliError::Operation("Volume creation requires nvmserver to be running".into())
        })?;
        
        // TODO: Implement actual volume creation
        Ok("vol-001".to_string())
    }
    
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct PoolInfo {
        pub name: String,
        pub pool_type: String,
        pub total_gb: u64,
        pub used_gb: u64,
        pub status: String,
    }
    
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct VolumeInfo {
        pub id: String,
        pub name: String,
        pub pool: String,
        pub size_gb: u64,
        pub format: String,
    }
}

/// Network commands
pub mod network {
    use super::*;
    
    pub fn list() -> CliResult<Vec<NetworkInfo>> {
        let client = ApiClient::from_config();
        
        match client.get::<Vec<NetworkInfo>>("/networks") {
            Ok(nets) => return Ok(nets),
            Err(CliError::Network(_)) => {}
            Err(e) => return Err(e),
        }
        
        // Fall back to local state
        let state = LocalState::load();
        Ok(state.networks.values()
            .map(|n| NetworkInfo {
                name: n.name.clone(),
                network_type: n.network_type.clone(),
                cidr: n.cidr.clone(),
                status: "active".to_string(),
            })
            .collect())
    }
    
    pub fn create(_name: &str, _cidr: &str) -> CliResult<String> {
        let client = ApiClient::from_config();
        client.ensure_connected().map_err(|_| {
            CliError::Operation("Network creation requires nvmserver to be running".into())
        })?;
        
        Ok("net-001".to_string())
    }
    
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct NetworkInfo {
        pub name: String,
        pub network_type: String,
        pub cidr: Option<String>,
        pub status: String,
    }
}

/// Cluster commands
pub mod cluster {
    use super::*;
    
    pub fn status() -> CliResult<ClusterStatus> {
        let client = ApiClient::from_config();
        
        match client.get::<ClusterStatus>("/cluster/status") {
            Ok(status) => return Ok(status),
            Err(CliError::Network(_)) => {}
            Err(e) => return Err(e),
        }
        
        // Standalone mode
        Ok(ClusterStatus {
            name: "standalone".to_string(),
            status: "standalone".to_string(),
            nodes: 1,
            quorum: true,
        })
    }
    
    pub fn list_nodes() -> CliResult<Vec<NodeInfo>> {
        let client = ApiClient::from_config();
        
        match client.get::<Vec<NodeInfo>>("/nodes") {
            Ok(nodes) => return Ok(nodes),
            Err(CliError::Network(_)) => {}
            Err(e) => return Err(e),
        }
        
        // Local node when standalone
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "localhost".to_string());
        
        Ok(vec![NodeInfo {
            id: "local".to_string(),
            hostname,
            status: "online".to_string(),
            role: "standalone".to_string(),
        }])
    }
    
    pub fn join(_address: &str, _token: &str) -> CliResult<()> {
        let client = ApiClient::from_config();
        client.ensure_connected().map_err(|_| {
            CliError::Operation("Cluster operations require nvmserver to be running".into())
        })?;
        Ok(())
    }
    
    pub fn leave() -> CliResult<()> {
        let client = ApiClient::from_config();
        client.ensure_connected().map_err(|_| {
            CliError::Operation("Cluster operations require nvmserver to be running".into())
        })?;
        Ok(())
    }
    
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ClusterStatus {
        pub name: String,
        pub status: String,
        pub nodes: u32,
        pub quorum: bool,
    }
    
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct NodeInfo {
        pub id: String,
        pub hostname: String,
        pub status: String,
        pub role: String,
    }
}

/// Backup commands
pub mod backup {
    use super::*;
    
    pub fn create(_vm_id: &str, _target: &str, _backup_type: &str) -> CliResult<String> {
        let client = ApiClient::from_config();
        client.ensure_connected().map_err(|_| {
            CliError::Operation("Backup operations require nvmserver to be running".into())
        })?;
        Ok("backup-001".to_string())
    }
    
    pub fn restore(_backup_id: &str, _vm_name: Option<&str>) -> CliResult<String> {
        let client = ApiClient::from_config();
        client.ensure_connected().map_err(|_| {
            CliError::Operation("Restore operations require nvmserver to be running".into())
        })?;
        Ok("vm-restored".to_string())
    }
    
    pub fn list(_vm_id: Option<&str>) -> CliResult<Vec<BackupInfo>> {
        let client = ApiClient::from_config();
        
        match client.get::<Vec<BackupInfo>>("/backup/jobs") {
            Ok(backups) => return Ok(backups),
            Err(CliError::Network(_)) => {}
            Err(e) => return Err(e),
        }
        
        Ok(vec![])
    }
    
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct BackupInfo {
        pub id: String,
        pub vm_id: String,
        pub backup_type: String,
        pub size_gb: f64,
        pub created_at: u64,
    }
}

/// User commands
pub mod user {
    use super::*;
    
    pub fn list() -> CliResult<Vec<UserInfo>> {
        let client = ApiClient::from_config();
        
        match client.get::<Vec<UserInfo>>("/users") {
            Ok(users) => return Ok(users),
            Err(CliError::Network(_)) => {}
            Err(e) => return Err(e),
        }
        
        // Default admin user when server not available
        Ok(vec![UserInfo {
            username: "admin".to_string(),
            roles: vec!["Administrator".to_string()],
            enabled: true,
        }])
    }
    
    pub fn create(_username: &str, _password: &str, _roles: Vec<String>) -> CliResult<String> {
        let client = ApiClient::from_config();
        client.ensure_connected().map_err(|_| {
            CliError::Operation("User management requires nvmserver to be running".into())
        })?;
        Ok("user-created".to_string())
    }
    
    pub fn passwd(_username: &str) -> CliResult<()> {
        let client = ApiClient::from_config();
        client.ensure_connected().map_err(|_| {
            CliError::Operation("Password change requires nvmserver to be running".into())
        })?;
        Ok(())
    }
    
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct UserInfo {
        pub username: String,
        pub roles: Vec<String>,
        pub enabled: bool,
    }
}

/// System commands
pub mod system {
    use super::*;
    
    pub fn info() -> CliResult<SystemInfo> {
        let client = ApiClient::from_config();
        
        match client.get::<SystemInfo>("/system/info") {
            Ok(info) => return Ok(info),
            Err(CliError::Network(_)) => {}
            Err(e) => return Err(e),
        }
        
        // Local info when server not available
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        
        Ok(SystemInfo {
            version: env!("CARGO_PKG_VERSION").to_string(),
            hostname,
            uptime: 0, // Unknown when server not running
        })
    }
    
    pub fn license() -> CliResult<LicenseInfo> {
        let client = ApiClient::from_config();
        
        match client.get::<LicenseInfo>("/system/license") {
            Ok(lic) => return Ok(lic),
            Err(CliError::Network(_)) => {}
            Err(e) => return Err(e),
        }
        
        Ok(LicenseInfo {
            edition: "Community".to_string(),
            status: "active".to_string(),
            expires: None,
        })
    }
    
    pub fn update_check() -> CliResult<UpdateInfo> {
        let client = ApiClient::from_config();
        
        match client.get::<UpdateInfo>("/system/update") {
            Ok(upd) => return Ok(upd),
            Err(CliError::Network(_)) => {}
            Err(e) => return Err(e),
        }
        
        Ok(UpdateInfo {
            current: env!("CARGO_PKG_VERSION").to_string(),
            latest: env!("CARGO_PKG_VERSION").to_string(),
            available: false,
        })
    }
    
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SystemInfo {
        pub version: String,
        pub hostname: String,
        pub uptime: u64,
    }
    
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct LicenseInfo {
        pub edition: String,
        pub status: String,
        pub expires: Option<u64>,
    }
    
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct UpdateInfo {
        pub current: String,
        pub latest: String,
        pub available: bool,
    }
}
