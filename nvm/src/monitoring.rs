//! Monitoring and Alerting System
//!
//! Enterprise-grade monitoring with:
//! - Real-time metrics collection
//! - Performance monitoring
//! - Health checks
//! - Alert management
//! - Historical data retention

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock, Mutex, atomic::{AtomicU64, Ordering}};
use std::time::{Instant, Duration};

/// Monitoring manager
pub struct MonitoringManager {
    /// Metrics collectors
    collectors: RwLock<HashMap<String, Arc<MetricsCollector>>>,
    /// Alert manager
    alerts: Arc<AlertManager>,
    /// Health checks
    health_checks: RwLock<Vec<HealthCheck>>,
    /// Configuration
    config: RwLock<MonitoringConfig>,
    /// Global statistics
    stats: RwLock<MonitoringStats>,
}

/// Monitoring configuration
#[derive(Debug, Clone)]
pub struct MonitoringConfig {
    /// Metrics collection interval (seconds)
    pub collection_interval: u64,
    /// Metrics retention period (hours)
    pub retention_hours: u64,
    /// Enable detailed tracing
    pub detailed_tracing: bool,
    /// Health check interval (seconds)
    pub health_check_interval: u64,
    /// Alert cooldown period (seconds)
    pub alert_cooldown: u64,
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self {
            collection_interval: 10,
            retention_hours: 168, // 1 week
            detailed_tracing: false,
            health_check_interval: 30,
            alert_cooldown: 300,
        }
    }
}

/// Monitoring statistics
#[derive(Debug, Clone, Default)]
pub struct MonitoringStats {
    pub metrics_collected: u64,
    pub alerts_fired: u64,
    pub health_checks_performed: u64,
}

impl MonitoringManager {
    pub fn new() -> Self {
        Self {
            collectors: RwLock::new(HashMap::new()),
            alerts: Arc::new(AlertManager::new()),
            health_checks: RwLock::new(Vec::new()),
            config: RwLock::new(MonitoringConfig::default()),
            stats: RwLock::new(MonitoringStats::default()),
        }
    }
    
    /// Configure monitoring
    pub fn configure(&self, config: MonitoringConfig) {
        *self.config.write().unwrap() = config;
    }
    
    /// Register a metrics collector
    pub fn register_collector(&self, name: &str, collector: Arc<MetricsCollector>) {
        self.collectors.write().unwrap().insert(name.to_string(), collector);
    }
    
    /// Get metrics collector
    pub fn get_collector(&self, name: &str) -> Option<Arc<MetricsCollector>> {
        self.collectors.read().unwrap().get(name).cloned()
    }
    
    /// Get alert manager
    pub fn alerts(&self) -> Arc<AlertManager> {
        self.alerts.clone()
    }
    
    /// Add health check
    pub fn add_health_check(&self, check: HealthCheck) {
        self.health_checks.write().unwrap().push(check);
    }
    
    /// Run all health checks
    pub fn run_health_checks(&self) -> Vec<HealthStatus> {
        let checks = self.health_checks.read().unwrap();
        let mut results = Vec::new();
        
        for check in checks.iter() {
            let status = (check.check_fn)();
            
            if !status.healthy {
                // Fire alert if unhealthy
                self.alerts.fire(Alert {
                    id: 0,
                    severity: AlertSeverity::Critical,
                    source: check.name.clone(),
                    message: format!("Health check failed: {}", status.message),
                    timestamp: Instant::now(),
                    acknowledged: false,
                    resolved: false,
                });
            }
            
            results.push(status);
        }
        
        self.stats.write().unwrap().health_checks_performed += 1;
        results
    }
    
    /// Collect all metrics
    pub fn collect_metrics(&self) -> HashMap<String, PerformanceMetrics> {
        let collectors = self.collectors.read().unwrap();
        let mut all_metrics = HashMap::new();
        
        for (name, collector) in collectors.iter() {
            all_metrics.insert(name.clone(), collector.collect());
        }
        
        self.stats.write().unwrap().metrics_collected += 1;
        all_metrics
    }
}

impl Default for MonitoringManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Metrics collector
pub struct MetricsCollector {
    /// Collector name
    name: String,
    /// Metrics history
    history: RwLock<VecDeque<MetricDataPoint>>,
    /// Maximum history size
    max_history: usize,
    /// Current metrics
    current: RwLock<PerformanceMetrics>,
}

/// Metric data point
#[derive(Debug, Clone)]
pub struct MetricDataPoint {
    pub timestamp: Instant,
    pub metrics: PerformanceMetrics,
}

/// Performance metrics
#[derive(Debug, Clone, Default)]
pub struct PerformanceMetrics {
    // CPU metrics
    pub cpu_usage_percent: f64,
    pub cpu_steal_percent: f64,
    pub cpu_wait_percent: f64,
    
    // Memory metrics
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub memory_cached_bytes: u64,
    pub memory_balloon_bytes: u64,
    pub swap_used_bytes: u64,
    
    // Disk metrics
    pub disk_read_bytes: u64,
    pub disk_write_bytes: u64,
    pub disk_read_ops: u64,
    pub disk_write_ops: u64,
    pub disk_latency_us: u64,
    
    // Network metrics
    pub net_rx_bytes: u64,
    pub net_tx_bytes: u64,
    pub net_rx_packets: u64,
    pub net_tx_packets: u64,
    pub net_rx_errors: u64,
    pub net_tx_errors: u64,
    
    // VM specific
    pub vm_exits_per_sec: u64,
    pub interrupts_per_sec: u64,
    pub page_faults_per_sec: u64,
}

impl MetricsCollector {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            history: RwLock::new(VecDeque::new()),
            max_history: 8640, // 24 hours at 10s intervals
            current: RwLock::new(PerformanceMetrics::default()),
        }
    }
    
    /// Record metrics
    pub fn record(&self, metrics: PerformanceMetrics) {
        let mut history = self.history.write().unwrap();
        
        // Add to history
        history.push_back(MetricDataPoint {
            timestamp: Instant::now(),
            metrics: metrics.clone(),
        });
        
        // Trim old entries
        while history.len() > self.max_history {
            history.pop_front();
        }
        
        // Update current
        *self.current.write().unwrap() = metrics;
    }
    
    /// Collect current metrics
    pub fn collect(&self) -> PerformanceMetrics {
        self.current.read().unwrap().clone()
    }
    
    /// Get history
    pub fn get_history(&self, duration: Duration) -> Vec<MetricDataPoint> {
        let history = self.history.read().unwrap();
        let cutoff = Instant::now() - duration;
        
        history.iter()
            .filter(|dp| dp.timestamp >= cutoff)
            .cloned()
            .collect()
    }
    
    /// Calculate average over duration
    pub fn average(&self, duration: Duration) -> PerformanceMetrics {
        let history = self.get_history(duration);
        if history.is_empty() {
            return PerformanceMetrics::default();
        }
        
        let count = history.len() as f64;
        let mut avg = PerformanceMetrics::default();
        
        for dp in history {
            avg.cpu_usage_percent += dp.metrics.cpu_usage_percent;
            avg.memory_used_bytes += dp.metrics.memory_used_bytes;
            avg.disk_read_bytes += dp.metrics.disk_read_bytes;
            avg.disk_write_bytes += dp.metrics.disk_write_bytes;
            avg.net_rx_bytes += dp.metrics.net_rx_bytes;
            avg.net_tx_bytes += dp.metrics.net_tx_bytes;
        }
        
        avg.cpu_usage_percent /= count;
        avg.memory_used_bytes = (avg.memory_used_bytes as f64 / count) as u64;
        avg.disk_read_bytes = (avg.disk_read_bytes as f64 / count) as u64;
        avg.disk_write_bytes = (avg.disk_write_bytes as f64 / count) as u64;
        avg.net_rx_bytes = (avg.net_rx_bytes as f64 / count) as u64;
        avg.net_tx_bytes = (avg.net_tx_bytes as f64 / count) as u64;
        
        avg
    }
}

/// Metric type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricType {
    Gauge,
    Counter,
    Histogram,
    Summary,
}

/// Alert manager
pub struct AlertManager {
    /// Active alerts
    alerts: RwLock<Vec<Alert>>,
    /// Alert rules
    rules: RwLock<Vec<AlertRule>>,
    /// Next alert ID
    next_id: AtomicU64,
    /// Alert history
    history: RwLock<VecDeque<Alert>>,
    /// Subscribers
    subscribers: RwLock<Vec<Box<dyn AlertSubscriber + Send + Sync>>>,
}

/// Alert
#[derive(Debug, Clone)]
pub struct Alert {
    pub id: u64,
    pub severity: AlertSeverity,
    pub source: String,
    pub message: String,
    pub timestamp: Instant,
    pub acknowledged: bool,
    pub resolved: bool,
}

/// Alert severity
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlertSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

/// Alert rule
#[derive(Clone)]
pub struct AlertRule {
    pub name: String,
    pub condition: AlertCondition,
    pub severity: AlertSeverity,
    pub message_template: String,
    pub cooldown: Duration,
    pub last_fired: Option<Instant>,
}

/// Alert condition
#[derive(Clone)]
pub enum AlertCondition {
    /// Metric exceeds threshold
    Threshold { metric: String, operator: ThresholdOperator, value: f64 },
    /// Metric rate of change
    RateOfChange { metric: String, period: Duration, threshold: f64 },
    /// Health check failure
    HealthCheckFailed { check_name: String },
    /// Custom condition
    Custom(Arc<dyn Fn(&PerformanceMetrics) -> bool + Send + Sync>),
}

/// Threshold operator
#[derive(Debug, Clone, Copy)]
pub enum ThresholdOperator {
    GreaterThan,
    LessThan,
    Equal,
    NotEqual,
}

/// Alert subscriber trait
pub trait AlertSubscriber {
    fn on_alert(&self, alert: &Alert);
}

impl AlertManager {
    pub fn new() -> Self {
        Self {
            alerts: RwLock::new(Vec::new()),
            rules: RwLock::new(Vec::new()),
            next_id: AtomicU64::new(1),
            history: RwLock::new(VecDeque::new()),
            subscribers: RwLock::new(Vec::new()),
        }
    }
    
    /// Add alert rule
    pub fn add_rule(&self, rule: AlertRule) {
        self.rules.write().unwrap().push(rule);
    }
    
    /// Fire an alert
    pub fn fire(&self, mut alert: Alert) {
        alert.id = self.next_id.fetch_add(1, Ordering::SeqCst);
        
        // Notify subscribers
        let subscribers = self.subscribers.read().unwrap();
        for sub in subscribers.iter() {
            sub.on_alert(&alert);
        }
        
        // Add to active alerts
        self.alerts.write().unwrap().push(alert.clone());
        
        // Add to history
        let mut history = self.history.write().unwrap();
        history.push_back(alert);
        while history.len() > 10000 {
            history.pop_front();
        }
    }
    
    /// Acknowledge alert
    pub fn acknowledge(&self, id: u64) {
        if let Some(alert) = self.alerts.write().unwrap().iter_mut().find(|a| a.id == id) {
            alert.acknowledged = true;
        }
    }
    
    /// Resolve alert
    pub fn resolve(&self, id: u64) {
        let mut alerts = self.alerts.write().unwrap();
        if let Some(alert) = alerts.iter_mut().find(|a| a.id == id) {
            alert.resolved = true;
        }
        // Remove resolved alerts
        alerts.retain(|a| !a.resolved);
    }
    
    /// Get active alerts
    pub fn active_alerts(&self) -> Vec<Alert> {
        self.alerts.read().unwrap().clone()
    }
    
    /// Get alerts by severity
    pub fn alerts_by_severity(&self, min_severity: AlertSeverity) -> Vec<Alert> {
        self.alerts.read().unwrap()
            .iter()
            .filter(|a| a.severity >= min_severity)
            .cloned()
            .collect()
    }
    
    /// Subscribe to alerts
    pub fn subscribe(&self, subscriber: Box<dyn AlertSubscriber + Send + Sync>) {
        self.subscribers.write().unwrap().push(subscriber);
    }
    
    /// Check rules against metrics
    pub fn check_rules(&self, metrics: &PerformanceMetrics) {
        let alert_to_fire = {
            let mut rules = self.rules.write().unwrap();
            let mut result = None;
            
            for rule in rules.iter_mut() {
                // Check cooldown
                if let Some(last) = rule.last_fired {
                    if last.elapsed() < rule.cooldown {
                        continue;
                    }
                }
                
                // Evaluate condition
                let triggered = match &rule.condition {
                    AlertCondition::Threshold { metric, operator, value } => {
                        let metric_value = match metric.as_str() {
                            "cpu_usage" => metrics.cpu_usage_percent,
                            "memory_used" => metrics.memory_used_bytes as f64,
                            _ => 0.0,
                        };
                        match operator {
                            ThresholdOperator::GreaterThan => metric_value > *value,
                            ThresholdOperator::LessThan => metric_value < *value,
                            ThresholdOperator::Equal => (metric_value - value).abs() < f64::EPSILON,
                            ThresholdOperator::NotEqual => (metric_value - value).abs() >= f64::EPSILON,
                        }
                    }
                    AlertCondition::Custom(f) => f(metrics),
                    _ => false,
                };
                
                if triggered {
                    rule.last_fired = Some(Instant::now());
                    result = Some(Alert {
                        id: 0,
                        severity: rule.severity,
                        source: rule.name.clone(),
                        message: rule.message_template.clone(),
                        timestamp: Instant::now(),
                        acknowledged: false,
                        resolved: false,
                    });
                    break;
                }
            }
            result
        };
        
        if let Some(alert) = alert_to_fire {
            self.fire(alert);
        }
    }
}

impl Default for AlertManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Health check
pub struct HealthCheck {
    pub name: String,
    pub check_fn: Box<dyn Fn() -> HealthStatus + Send + Sync>,
}

/// Health status
#[derive(Debug, Clone)]
pub struct HealthStatus {
    pub name: String,
    pub healthy: bool,
    pub message: String,
    pub timestamp: Instant,
    pub details: HashMap<String, String>,
}

impl HealthStatus {
    pub fn healthy(name: &str) -> Self {
        Self {
            name: name.to_string(),
            healthy: true,
            message: "OK".to_string(),
            timestamp: Instant::now(),
            details: HashMap::new(),
        }
    }
    
    pub fn unhealthy(name: &str, message: &str) -> Self {
        Self {
            name: name.to_string(),
            healthy: false,
            message: message.to_string(),
            timestamp: Instant::now(),
            details: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_metrics_collector() {
        let collector = MetricsCollector::new("test");
        
        let metrics = PerformanceMetrics {
            cpu_usage_percent: 50.0,
            memory_used_bytes: 1024 * 1024 * 1024,
            ..Default::default()
        };
        
        collector.record(metrics.clone());
        
        let collected = collector.collect();
        assert_eq!(collected.cpu_usage_percent, 50.0);
    }
    
    #[test]
    fn test_alert_manager() {
        let alerts = AlertManager::new();
        
        alerts.fire(Alert {
            id: 0,
            severity: AlertSeverity::Warning,
            source: "test".to_string(),
            message: "Test alert".to_string(),
            timestamp: Instant::now(),
            acknowledged: false,
            resolved: false,
        });
        
        let active = alerts.active_alerts();
        assert_eq!(active.len(), 1);
        
        let id = active[0].id;
        alerts.resolve(id);
        
        assert!(alerts.active_alerts().is_empty());
    }
}
