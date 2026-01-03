//! System handlers
//!
//! System information using real host data

use super::ApiResponse;
use crate::webgui::server::WebGuiState;
use std::sync::Arc;
use serde::{Deserialize, Serialize};

#[cfg(feature = "webgui")]
use axum::{
    extract::{State, Json},
    http::StatusCode,
    response::IntoResponse,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub product: String,
    pub version: String,
    pub build: String,
    pub hostname: String,
    pub node_id: String,
    pub cluster_name: Option<String>,
    pub uptime: u64,
    pub os: OsInfo,
    pub hardware: HardwareInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsInfo {
    pub name: String,
    pub version: String,
    pub kernel: String,
    pub arch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareInfo {
    pub cpu_model: String,
    pub cpu_cores: u32,
    pub cpu_threads: u32,
    pub memory_total_gb: u64,
    pub virtualization: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfig {
    pub network: NetworkConfig,
    pub time: TimeConfig,
    pub logging: LoggingConfig,
    pub security: SecurityConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub hostname: String,
    pub domain: Option<String>,
    pub dns_servers: Vec<String>,
    pub ntp_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeConfig {
    pub timezone: String,
    pub ntp_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub retention_days: u32,
    pub remote_syslog: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub two_factor_required: bool,
    pub session_timeout_minutes: u32,
    pub password_policy: PasswordPolicy,
    pub firewall_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordPolicy {
    pub min_length: u32,
    pub require_uppercase: bool,
    pub require_numbers: bool,
    pub require_special: bool,
    pub max_age_days: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseInfo {
    pub product: String,
    pub edition: String,
    pub license_key: Option<String>,
    pub licensed_to: Option<String>,
    pub valid_until: Option<u64>,
    pub features: Vec<String>,
    pub limits: LicenseLimits,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseLimits {
    pub max_nodes: Option<u32>,
    pub max_vms: Option<u32>,
    pub max_memory_gb: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivateLicenseRequest {
    pub license_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release_notes: Option<String>,
    pub download_url: Option<String>,
}

#[cfg(feature = "webgui")]
pub async fn info(
    State(_state): State<Arc<WebGuiState>>,
) -> impl IntoResponse {
    // Get real hostname
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "localhost".to_string());
    
    // Get real CPU info
    let cpu_cores = num_cpus::get() as u32;
    let cpu_threads = num_cpus::get() as u32; // Physical cores on Linux
    
    // Get CPU model
    #[cfg(target_os = "linux")]
    let cpu_model = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split(':').nth(1))
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| "Unknown CPU".to_string());
    #[cfg(not(target_os = "linux"))]
    let cpu_model = "Unknown CPU".to_string();
    
    // Get memory total
    #[cfg(target_os = "linux")]
    let memory_total_gb = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("MemTotal:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<u64>().ok())
                .map(|kb| kb / 1024 / 1024)
        })
        .unwrap_or(16);
    #[cfg(not(target_os = "linux"))]
    let memory_total_gb = 16u64;
    
    // Get uptime
    #[cfg(target_os = "linux")]
    let uptime = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
        .map(|s| s as u64)
        .unwrap_or(0);
    #[cfg(not(target_os = "linux"))]
    let uptime = 0u64;
    
    // Get OS info
    #[cfg(target_os = "linux")]
    let (os_name, os_version) = std::fs::read_to_string("/etc/os-release")
        .ok()
        .map(|s| {
            let name = s.lines()
                .find(|l| l.starts_with("NAME="))
                .and_then(|l| l.split('=').nth(1))
                .map(|s| s.trim_matches('"').to_string())
                .unwrap_or_else(|| "Linux".to_string());
            let version = s.lines()
                .find(|l| l.starts_with("VERSION_ID="))
                .and_then(|l| l.split('=').nth(1))
                .map(|s| s.trim_matches('"').to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            (name, version)
        })
        .unwrap_or_else(|| ("Linux".to_string(), "Unknown".to_string()));
    #[cfg(not(target_os = "linux"))]
    let (os_name, os_version) = ("Unknown".to_string(), "Unknown".to_string());
    
    // Get kernel version
    #[cfg(target_os = "linux")]
    let kernel_version = std::fs::read_to_string("/proc/version")
        .ok()
        .and_then(|s| s.split_whitespace().nth(2).map(|s| s.to_string()))
        .unwrap_or_else(|| "Unknown".to_string());
    #[cfg(not(target_os = "linux"))]
    let kernel_version = "Unknown".to_string();
    
    // Detect virtualization capabilities
    #[cfg(target_os = "linux")]
    let virtualization = {
        let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        if cpuinfo.contains("vmx") {
            "Intel VT-x".to_string()
        } else if cpuinfo.contains("svm") {
            "AMD-V".to_string()
        } else {
            "None detected".to_string()
        }
    };
    #[cfg(not(target_os = "linux"))]
    let virtualization = "Unknown".to_string();
    
    let info = SystemInfo {
        product: "NexaOS Virtual Machine Manager".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        build: chrono::Utc::now().format("%Y%m%d").to_string(),
        hostname: hostname.clone(),
        node_id: format!("node-{}", &hostname[..hostname.len().min(8)]),
        cluster_name: Some("local-cluster".to_string()),
        uptime,
        os: OsInfo {
            name: os_name,
            version: os_version,
            kernel: kernel_version,
            arch: std::env::consts::ARCH.to_string(),
        },
        hardware: HardwareInfo {
            cpu_model,
            cpu_cores,
            cpu_threads,
            memory_total_gb,
            virtualization,
        },
    };
    
    Json(ApiResponse::success(info))
}

#[cfg(feature = "webgui")]
pub async fn config(
    State(_state): State<Arc<WebGuiState>>,
) -> impl IntoResponse {
    // Get real hostname and DNS
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "localhost".to_string());
    
    // Get DNS servers from resolv.conf
    #[cfg(target_os = "linux")]
    let dns_servers: Vec<String> = std::fs::read_to_string("/etc/resolv.conf")
        .ok()
        .map(|s| {
            s.lines()
                .filter(|l| l.starts_with("nameserver"))
                .filter_map(|l| l.split_whitespace().nth(1))
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_else(|| vec!["8.8.8.8".to_string()]);
    #[cfg(not(target_os = "linux"))]
    let dns_servers = vec!["8.8.8.8".to_string(), "8.8.4.4".to_string()];
    
    // Get timezone
    #[cfg(target_os = "linux")]
    let timezone = std::fs::read_link("/etc/localtime")
        .ok()
        .and_then(|p| {
            p.to_string_lossy()
                .split("/zoneinfo/")
                .last()
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "UTC".to_string());
    #[cfg(not(target_os = "linux"))]
    let timezone = "UTC".to_string();
    
    let config = SystemConfig {
        network: NetworkConfig {
            hostname,
            domain: None,
            dns_servers,
            ntp_servers: vec!["pool.ntp.org".to_string()],
        },
        time: TimeConfig {
            timezone,
            ntp_enabled: true,
        },
        logging: LoggingConfig {
            level: "info".to_string(),
            retention_days: 30,
            remote_syslog: None,
        },
        security: SecurityConfig {
            two_factor_required: false,
            session_timeout_minutes: 480,
            password_policy: PasswordPolicy {
                min_length: 8,
                require_uppercase: true,
                require_numbers: true,
                require_special: false,
                max_age_days: 90,
            },
            firewall_enabled: true,
        },
    };
    
    Json(ApiResponse::success(config))
}

#[cfg(feature = "webgui")]
pub async fn update_config(
    State(_state): State<Arc<WebGuiState>>,
    Json(_config): Json<SystemConfig>,
) -> impl IntoResponse {
    // In real implementation, apply configuration changes
    Json(ApiResponse::success(serde_json::json!({"updated": true})))
}

#[cfg(feature = "webgui")]
pub async fn license(
    State(_state): State<Arc<WebGuiState>>,
) -> impl IntoResponse {
    use crate::vmstate::vm_state;
    
    let state_mgr = vm_state();
    let vm_count = state_mgr.list_vms().len() as u32;
    
    let license = LicenseInfo {
        product: "NexaOS Virtual Machine Manager".to_string(),
        edition: "Community".to_string(),
        license_key: None,
        licensed_to: None,
        valid_until: None,
        features: vec![
            "basic_vm_management".to_string(),
            "snapshots".to_string(),
            "backup".to_string(),
            "templates".to_string(),
        ],
        limits: LicenseLimits {
            max_nodes: Some(3),
            max_vms: Some(10),
            max_memory_gb: Some(64),
        },
        status: if vm_count <= 10 { "valid" } else { "exceeded" }.to_string(),
    };
    
    Json(ApiResponse::success(license))
}

#[cfg(feature = "webgui")]
pub async fn activate_license(
    State(_state): State<Arc<WebGuiState>>,
    Json(_req): Json<ActivateLicenseRequest>,
) -> impl IntoResponse {
    // In real implementation, validate and activate license
    Json(ApiResponse::success(serde_json::json!({
        "activated": true,
        "edition": "Enterprise",
        "valid_until": chrono::Utc::now().timestamp() as u64 + 86400 * 365
    })))
}

#[cfg(feature = "webgui")]
pub async fn check_updates(
    State(_state): State<Arc<WebGuiState>>,
) -> impl IntoResponse {
    let current_version = env!("CARGO_PKG_VERSION");
    
    let update = UpdateInfo {
        current_version: current_version.to_string(),
        latest_version: current_version.to_string(),
        update_available: false,
        release_notes: None,
        download_url: None,
    };
    
    Json(ApiResponse::success(update))
}
