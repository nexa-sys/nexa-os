//! Event Persistence - Store and retrieve events

use super::{Event, EventCategory, EventFilter, EventId, EventSeverity};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
use parking_lot::RwLock;

/// Event storage backend trait
pub trait EventStore: Send + Sync {
    /// Store an event
    fn store(&self, event: &Event) -> Result<(), StoreError>;
    
    /// Get event by ID
    fn get(&self, id: EventId) -> Result<Option<Event>, StoreError>;
    
    /// Query events
    fn query(&self, query: &EventQuery) -> Result<Vec<Event>, StoreError>;
    
    /// Delete old events
    fn prune(&self, before: u64) -> Result<usize, StoreError>;
    
    /// Get event count
    fn count(&self) -> Result<usize, StoreError>;
}

/// Storage error
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(String),
    
    #[error("Database error: {0}")]
    Database(String),
    
    #[error("Not found")]
    NotFound,
}

/// Event query parameters
#[derive(Debug, Clone, Default)]
pub struct EventQuery {
    pub start_time: Option<u64>,
    pub end_time: Option<u64>,
    pub categories: Option<Vec<EventCategory>>,
    pub min_severity: Option<EventSeverity>,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
    pub node_id: Option<String>,
    pub user: Option<String>,
    pub event_type: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub order_desc: bool,
}

/// In-memory event store (for development/small deployments)
pub struct MemoryStore {
    events: RwLock<VecDeque<Event>>,
    max_size: usize,
}

impl MemoryStore {
    pub fn new(max_size: usize) -> Self {
        Self {
            events: RwLock::new(VecDeque::with_capacity(max_size)),
            max_size,
        }
    }
}

impl EventStore for MemoryStore {
    fn store(&self, event: &Event) -> Result<(), StoreError> {
        let mut events = self.events.write();
        
        if events.len() >= self.max_size {
            events.pop_front();
        }
        
        events.push_back(event.clone());
        Ok(())
    }

    fn get(&self, id: EventId) -> Result<Option<Event>, StoreError> {
        let events = self.events.read();
        Ok(events.iter().find(|e| e.id == id).cloned())
    }

    fn query(&self, query: &EventQuery) -> Result<Vec<Event>, StoreError> {
        let events = self.events.read();
        
        let mut results: Vec<Event> = events
            .iter()
            .filter(|e| {
                // Time filter
                if let Some(start) = query.start_time {
                    if e.timestamp < start {
                        return false;
                    }
                }
                if let Some(end) = query.end_time {
                    if e.timestamp > end {
                        return false;
                    }
                }
                
                // Category filter
                if let Some(cats) = &query.categories {
                    if !cats.contains(&e.category) {
                        return false;
                    }
                }
                
                // Severity filter
                if let Some(min) = &query.min_severity {
                    let e_level = severity_level(e.severity);
                    let min_level = severity_level(*min);
                    if e_level < min_level {
                        return false;
                    }
                }
                
                // Resource filters
                if let Some(rt) = &query.resource_type {
                    if e.resource_type.as_ref() != Some(rt) {
                        return false;
                    }
                }
                if let Some(rid) = &query.resource_id {
                    if e.resource_id.as_ref() != Some(rid) {
                        return false;
                    }
                }
                
                // Node filter
                if let Some(nid) = &query.node_id {
                    if e.node_id.as_ref() != Some(nid) {
                        return false;
                    }
                }
                
                // User filter
                if let Some(u) = &query.user {
                    if e.user.as_ref() != Some(u) {
                        return false;
                    }
                }
                
                // Event type filter
                if let Some(et) = &query.event_type {
                    if !e.event_type.contains(et) {
                        return false;
                    }
                }
                
                true
            })
            .cloned()
            .collect();
        
        // Sort
        if query.order_desc {
            results.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        } else {
            results.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        }
        
        // Pagination
        let offset = query.offset.unwrap_or(0);
        let limit = query.limit.unwrap_or(usize::MAX);
        
        Ok(results.into_iter().skip(offset).take(limit).collect())
    }

    fn prune(&self, before: u64) -> Result<usize, StoreError> {
        let mut events = self.events.write();
        let original_len = events.len();
        events.retain(|e| e.timestamp >= before);
        Ok(original_len - events.len())
    }

    fn count(&self) -> Result<usize, StoreError> {
        Ok(self.events.read().len())
    }
}

fn severity_level(sev: EventSeverity) -> u8 {
    match sev {
        EventSeverity::Debug => 0,
        EventSeverity::Info => 1,
        EventSeverity::Warning => 2,
        EventSeverity::Error => 3,
        EventSeverity::Critical => 4,
    }
}

/// File-based event store (append-only log)
pub struct FileStore {
    path: PathBuf,
    memory_cache: MemoryStore,
}

impl FileStore {
    pub fn new(path: PathBuf, cache_size: usize) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        Ok(Self {
            path,
            memory_cache: MemoryStore::new(cache_size),
        })
    }

    fn append_to_file(&self, event: &Event) -> Result<(), StoreError> {
        use std::io::Write;
        
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        
        let json = serde_json::to_string(event)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        
        writeln!(file, "{}", json)?;
        Ok(())
    }
}

impl EventStore for FileStore {
    fn store(&self, event: &Event) -> Result<(), StoreError> {
        self.append_to_file(event)?;
        self.memory_cache.store(event)?;
        Ok(())
    }

    fn get(&self, id: EventId) -> Result<Option<Event>, StoreError> {
        // Try cache first
        if let Some(event) = self.memory_cache.get(id)? {
            return Ok(Some(event));
        }
        
        // Fall back to file scan (expensive)
        use std::io::BufRead;
        let file = std::fs::File::open(&self.path)?;
        let reader = std::io::BufReader::new(file);
        
        for line in reader.lines() {
            let line = line?;
            if let Ok(event) = serde_json::from_str::<Event>(&line) {
                if event.id == id {
                    return Ok(Some(event));
                }
            }
        }
        
        Ok(None)
    }

    fn query(&self, query: &EventQuery) -> Result<Vec<Event>, StoreError> {
        // For simplicity, delegate to memory cache
        // In production, implement proper file scanning
        self.memory_cache.query(query)
    }

    fn prune(&self, before: u64) -> Result<usize, StoreError> {
        self.memory_cache.prune(before)
    }

    fn count(&self) -> Result<usize, StoreError> {
        self.memory_cache.count()
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new(10000)
    }
}
