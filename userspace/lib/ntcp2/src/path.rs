//! Path Validation
//!
//! This module implements QUIC path validation (RFC 9000 Section 8).
//!
//! ## Overview
//!
//! Path validation is used to verify that the peer can receive packets
//! at the claimed network address. It's used for:
//! - Initial path validation during handshake
//! - Connection migration
//! - NAT rebinding detection
//!
//! ## Process
//!
//! 1. Send PATH_CHALLENGE with random 8-byte data
//! 2. Receive PATH_RESPONSE with same data
//! 3. Path is validated

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use crate::error::{Error, NgError, Result};

// ============================================================================
// Constants
// ============================================================================

/// Path challenge data size
pub const PATH_CHALLENGE_DATA_SIZE: usize = 8;

/// Maximum path validation attempts
pub const MAX_PATH_VALIDATION_ATTEMPTS: usize = 3;

/// Path validation timeout (initial, doubles each retry)
pub const PATH_VALIDATION_TIMEOUT: Duration = Duration::from_secs(3);

/// Maximum paths per connection
pub const MAX_PATHS: usize = 4;

// ============================================================================
// Types
// ============================================================================

/// Path challenge data
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PathChallengeData([u8; PATH_CHALLENGE_DATA_SIZE]);

impl PathChallengeData {
    /// Create new random challenge data
    pub fn new() -> Self {
        let mut data = [0u8; PATH_CHALLENGE_DATA_SIZE];
        // Use simple random generation
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = ((seed >> (i * 8)) & 0xff) as u8;
        }
        Self(data)
    }

    /// Create from bytes
    pub fn from_bytes(data: [u8; PATH_CHALLENGE_DATA_SIZE]) -> Self {
        Self(data)
    }

    /// Get bytes
    pub fn as_bytes(&self) -> &[u8; PATH_CHALLENGE_DATA_SIZE] {
        &self.0
    }
}

impl Default for PathChallengeData {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for PathChallengeData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PathChallengeData({:02x?})", &self.0[..])
    }
}

/// Path state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathState {
    /// Path not yet validated
    Unknown,
    /// Validation in progress
    Validating,
    /// Path validated successfully
    Validated,
    /// Validation failed
    Failed,
    /// Path is degraded (high loss/latency)
    Degraded,
}

/// Network path
#[derive(Debug, Clone)]
pub struct Path {
    /// Path ID
    pub id: PathId,
    /// Local address
    pub local: SocketAddr,
    /// Remote address
    pub remote: SocketAddr,
    /// Path state
    pub state: PathState,
    /// Current challenge data
    pub challenge: Option<PathChallengeData>,
    /// Pending responses to send
    pub pending_responses: Vec<PathChallengeData>,
    /// Validation attempts
    pub attempts: usize,
    /// Last validation attempt time
    pub last_attempt: Option<Instant>,
    /// Validation timeout
    pub timeout: Duration,
    /// RTT estimate (if validated)
    pub rtt: Option<Duration>,
    /// Congestion window
    pub cwnd: usize,
    /// Bytes in flight
    pub bytes_in_flight: usize,
    /// Is active path
    pub is_active: bool,
    /// MTU probe state
    pub mtu: usize,
    /// ECN capability
    pub ecn_capable: bool,
}

impl Path {
    /// Create a new path
    pub fn new(id: PathId, local: SocketAddr, remote: SocketAddr) -> Self {
        Self {
            id,
            local,
            remote,
            state: PathState::Unknown,
            challenge: None,
            pending_responses: Vec::new(),
            attempts: 0,
            last_attempt: None,
            timeout: PATH_VALIDATION_TIMEOUT,
            rtt: None,
            cwnd: 12000, // Initial CWND
            bytes_in_flight: 0,
            is_active: false,
            mtu: 1200, // Initial MTU
            ecn_capable: true, // Assume ECN capable until proven otherwise
        }
    }

    /// Start path validation
    pub fn start_validation(&mut self) {
        self.state = PathState::Validating;
        self.challenge = Some(PathChallengeData::new());
        self.attempts = 1;
        self.last_attempt = Some(Instant::now());
    }

    /// Retry validation
    pub fn retry_validation(&mut self) -> bool {
        if self.attempts >= MAX_PATH_VALIDATION_ATTEMPTS {
            self.state = PathState::Failed;
            return false;
        }

        self.challenge = Some(PathChallengeData::new());
        self.attempts += 1;
        self.last_attempt = Some(Instant::now());
        self.timeout *= 2; // Exponential backoff

        true
    }

    /// Handle challenge response
    pub fn handle_response(&mut self, data: &PathChallengeData) -> bool {
        if let Some(ref challenge) = self.challenge {
            if challenge == data {
                // Calculate RTT
                if let Some(start) = self.last_attempt {
                    self.rtt = Some(start.elapsed());
                }

                self.state = PathState::Validated;
                self.challenge = None;
                return true;
            }
        }
        false
    }

    /// Check if validation has timed out
    pub fn has_timed_out(&self) -> bool {
        if let Some(last) = self.last_attempt {
            last.elapsed() > self.timeout
        } else {
            false
        }
    }

    /// Add pending response
    pub fn add_pending_response(&mut self, data: PathChallengeData) {
        // Limit pending responses
        if self.pending_responses.len() < 8 {
            self.pending_responses.push(data);
        }
    }

    /// Get next pending response
    pub fn pop_pending_response(&mut self) -> Option<PathChallengeData> {
        if self.pending_responses.is_empty() {
            None
        } else {
            Some(self.pending_responses.remove(0))
        }
    }

    /// Check if path is usable
    pub fn is_usable(&self) -> bool {
        matches!(self.state, PathState::Validated | PathState::Degraded)
    }

    /// Mark path as degraded
    pub fn mark_degraded(&mut self) {
        if self.state == PathState::Validated {
            self.state = PathState::Degraded;
        }
    }

    /// Mark path as recovered
    pub fn mark_recovered(&mut self) {
        if self.state == PathState::Degraded {
            self.state = PathState::Validated;
        }
    }
}

/// Path identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PathId(pub u64);

impl PathId {
    /// Create from addresses
    pub fn from_addrs(local: &SocketAddr, remote: &SocketAddr) -> Self {
        // Simple hash of addresses
        let mut hash = 0u64;
        
        match local.ip() {
            IpAddr::V4(v4) => {
                hash ^= u32::from(v4) as u64;
            }
            IpAddr::V6(v6) => {
                for segment in v6.segments() {
                    hash ^= segment as u64;
                }
            }
        }
        hash ^= (local.port() as u64) << 16;

        match remote.ip() {
            IpAddr::V4(v4) => {
                hash ^= (u32::from(v4) as u64) << 32;
            }
            IpAddr::V6(v6) => {
                for segment in v6.segments() {
                    hash ^= (segment as u64) << 32;
                }
            }
        }
        hash ^= (remote.port() as u64) << 48;

        Self(hash)
    }
}

// ============================================================================
// Path Manager
// ============================================================================

/// Path manager
pub struct PathManager {
    /// All paths
    paths: HashMap<PathId, Path>,
    /// Active path ID
    active_path: Option<PathId>,
    /// Preferred path for sending
    preferred_path: Option<PathId>,
    /// Path ID counter
    next_id: u64,
    /// Allow migration
    allow_migration: bool,
}

impl PathManager {
    /// Create a new path manager
    pub fn new() -> Self {
        Self {
            paths: HashMap::new(),
            active_path: None,
            preferred_path: None,
            next_id: 0,
            allow_migration: true,
        }
    }

    /// Add a new path
    pub fn add_path(&mut self, local: SocketAddr, remote: SocketAddr) -> Result<PathId> {
        if self.paths.len() >= MAX_PATHS {
            return Err(Error::Ng(NgError::TooManyIds));
        }

        let id = PathId(self.next_id);
        self.next_id += 1;

        let path = Path::new(id, local, remote);
        self.paths.insert(id, path);

        // First path becomes active
        if self.active_path.is_none() {
            self.active_path = Some(id);
            if let Some(path) = self.paths.get_mut(&id) {
                path.is_active = true;
            }
        }

        Ok(id)
    }

    /// Get path by ID
    pub fn get_path(&self, id: PathId) -> Option<&Path> {
        self.paths.get(&id)
    }

    /// Get mutable path by ID
    pub fn get_path_mut(&mut self, id: PathId) -> Option<&mut Path> {
        self.paths.get_mut(&id)
    }

    /// Get active path
    pub fn active_path(&self) -> Option<&Path> {
        self.active_path.and_then(|id| self.paths.get(&id))
    }

    /// Get mutable active path
    pub fn active_path_mut(&mut self) -> Option<&mut Path> {
        let id = self.active_path?;
        self.paths.get_mut(&id)
    }

    /// Find path by addresses
    pub fn find_path(&self, local: &SocketAddr, remote: &SocketAddr) -> Option<PathId> {
        for (id, path) in &self.paths {
            if path.local == *local && path.remote == *remote {
                return Some(*id);
            }
            // Also check for matching remote only (NAT rebinding)
            if path.remote == *remote {
                return Some(*id);
            }
        }
        None
    }

    /// Start validation for a path
    pub fn start_validation(&mut self, id: PathId) -> Result<PathChallengeData> {
        let path = self.paths.get_mut(&id).ok_or(Error::Ng(NgError::Proto))?;
        path.start_validation();
        Ok(path.challenge.unwrap())
    }

    /// Handle PATH_CHALLENGE
    pub fn handle_challenge(
        &mut self,
        from: &SocketAddr,
        data: PathChallengeData,
    ) -> Option<PathId> {
        // Find or create path
        for (id, path) in self.paths.iter_mut() {
            if path.remote == *from {
                path.add_pending_response(data);
                return Some(*id);
            }
        }
        None
    }

    /// Handle PATH_RESPONSE
    pub fn handle_response(
        &mut self,
        from: &SocketAddr,
        data: PathChallengeData,
    ) -> Option<PathId> {
        for (id, path) in self.paths.iter_mut() {
            if path.remote == *from && path.handle_response(&data) {
                return Some(*id);
            }
        }
        None
    }

    /// Set active path
    pub fn set_active_path(&mut self, id: PathId) -> Result<()> {
        if !self.paths.contains_key(&id) {
            return Err(Error::Ng(NgError::Proto));
        }

        // Deactivate current
        if let Some(old_id) = self.active_path {
            if let Some(path) = self.paths.get_mut(&old_id) {
                path.is_active = false;
            }
        }

        // Activate new
        if let Some(path) = self.paths.get_mut(&id) {
            path.is_active = true;
        }
        self.active_path = Some(id);

        Ok(())
    }

    /// Remove a path
    pub fn remove_path(&mut self, id: PathId) -> Option<Path> {
        let path = self.paths.remove(&id)?;

        // Update active if needed
        if self.active_path == Some(id) {
            self.active_path = self.paths.keys().next().copied();
            if let Some(new_id) = self.active_path {
                if let Some(new_path) = self.paths.get_mut(&new_id) {
                    new_path.is_active = true;
                }
            }
        }

        Some(path)
    }

    /// Check for validation timeouts
    pub fn check_timeouts(&mut self) -> Vec<PathId> {
        let mut timed_out = Vec::new();

        for (id, path) in self.paths.iter_mut() {
            if path.state == PathState::Validating && path.has_timed_out() {
                if !path.retry_validation() {
                    timed_out.push(*id);
                }
            }
        }

        timed_out
    }

    /// Get validated paths
    pub fn validated_paths(&self) -> Vec<PathId> {
        self.paths
            .iter()
            .filter(|(_, p)| p.state == PathState::Validated)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Enable/disable migration
    pub fn set_allow_migration(&mut self, allow: bool) {
        self.allow_migration = allow;
    }

    /// Check if migration is allowed
    pub fn is_migration_allowed(&self) -> bool {
        self.allow_migration
    }

    /// Get path count
    pub fn path_count(&self) -> usize {
        self.paths.len()
    }

    /// Iterate over all paths
    pub fn iter(&self) -> impl Iterator<Item = (&PathId, &Path)> {
        self.paths.iter()
    }

    /// Get preferred path for sending
    pub fn get_send_path(&self) -> Option<&Path> {
        // Prefer explicitly set preferred path
        if let Some(id) = self.preferred_path {
            if let Some(path) = self.paths.get(&id) {
                if path.is_usable() {
                    return Some(path);
                }
            }
        }

        // Fall back to active path
        if let Some(path) = self.active_path() {
            if path.is_usable() {
                return Some(path);
            }
        }

        // Find any usable path
        self.paths.values().find(|p| p.is_usable())
    }

    /// Set preferred path
    pub fn set_preferred_path(&mut self, id: PathId) -> Result<()> {
        if !self.paths.contains_key(&id) {
            return Err(Error::Ng(NgError::Proto));
        }
        self.preferred_path = Some(id);
        Ok(())
    }
}

impl Default for PathManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// MTU Discovery
// ============================================================================

/// MTU discovery state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MtuState {
    /// Not probing
    Idle,
    /// Searching for MTU
    Searching,
    /// MTU found
    Found,
}

/// MTU discoverer
pub struct MtuDiscovery {
    /// Current state
    state: MtuState,
    /// Base MTU (known to work)
    base_mtu: usize,
    /// Current probe size
    probe_size: usize,
    /// Maximum possible MTU
    max_mtu: usize,
    /// Probe count
    probe_count: usize,
    /// Last probe time
    last_probe: Option<Instant>,
    /// Probe interval
    probe_interval: Duration,
}

impl MtuDiscovery {
    /// Create new MTU discovery
    pub fn new() -> Self {
        Self {
            state: MtuState::Idle,
            base_mtu: 1200,      // QUIC minimum
            probe_size: 1200,
            max_mtu: 65535,
            probe_count: 0,
            last_probe: None,
            probe_interval: Duration::from_secs(1),
        }
    }

    /// Start MTU discovery
    pub fn start(&mut self) {
        self.state = MtuState::Searching;
        self.probe_size = self.base_mtu + (self.max_mtu - self.base_mtu) / 2;
    }

    /// Get next probe size
    pub fn next_probe_size(&self) -> Option<usize> {
        if self.state != MtuState::Searching {
            return None;
        }

        // Check if enough time has passed
        if let Some(last) = self.last_probe {
            if last.elapsed() < self.probe_interval {
                return None;
            }
        }

        Some(self.probe_size)
    }

    /// Probe succeeded
    pub fn probe_succeeded(&mut self, size: usize) {
        self.base_mtu = size;
        
        // Binary search for larger MTU
        if self.probe_size < self.max_mtu - 10 {
            self.probe_size = self.base_mtu + (self.max_mtu - self.base_mtu) / 2;
        } else {
            self.state = MtuState::Found;
        }

        self.probe_count += 1;
        self.last_probe = Some(Instant::now());
    }

    /// Probe failed
    pub fn probe_failed(&mut self, size: usize) {
        self.max_mtu = size;

        // Binary search for smaller MTU
        if self.probe_size > self.base_mtu + 10 {
            self.probe_size = self.base_mtu + (self.max_mtu - self.base_mtu) / 2;
        } else {
            self.state = MtuState::Found;
        }

        self.probe_count += 1;
        self.last_probe = Some(Instant::now());
    }

    /// Get current MTU
    pub fn current_mtu(&self) -> usize {
        self.base_mtu
    }

    /// Get state
    pub fn state(&self) -> MtuState {
        self.state
    }

    /// Reset discovery
    pub fn reset(&mut self) {
        self.state = MtuState::Idle;
        self.probe_size = 1200;
        self.max_mtu = 65535;
        self.probe_count = 0;
        self.last_probe = None;
    }
}

impl Default for MtuDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Address Validation
// ============================================================================

/// Address validation token
#[derive(Clone)]
pub struct ValidationToken {
    /// Token data
    pub data: Vec<u8>,
    /// Creation time
    pub created: Instant,
    /// Client address
    pub client_addr: SocketAddr,
    /// Expiry duration
    pub lifetime: Duration,
}

impl ValidationToken {
    /// Create a new validation token
    pub fn new(client_addr: SocketAddr, secret: &[u8]) -> Self {
        let mut data = Vec::with_capacity(32);
        
        // Simple token: timestamp + address hash + HMAC
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        data.extend_from_slice(&now.to_be_bytes());
        
        // Address hash
        match client_addr.ip() {
            IpAddr::V4(v4) => {
                data.extend_from_slice(&v4.octets());
            }
            IpAddr::V6(v6) => {
                data.extend_from_slice(&v6.octets());
            }
        }
        data.extend_from_slice(&client_addr.port().to_be_bytes());
        
        // Simple HMAC (XOR with secret)
        for (i, byte) in data.iter_mut().enumerate() {
            if i < secret.len() {
                *byte ^= secret[i];
            }
        }

        Self {
            data,
            created: Instant::now(),
            client_addr,
            lifetime: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Check if token is expired
    pub fn is_expired(&self) -> bool {
        self.created.elapsed() > self.lifetime
    }

    /// Validate token
    pub fn validate(&self, addr: &SocketAddr, _secret: &[u8]) -> bool {
        // Check expiry
        if self.is_expired() {
            return false;
        }

        // Check address matches
        self.client_addr == *addr
    }
}

/// Address validator
pub struct AddressValidator {
    /// Secret for token generation
    secret: [u8; 32],
    /// Issued tokens
    tokens: Vec<ValidationToken>,
    /// Maximum tokens
    max_tokens: usize,
}

impl AddressValidator {
    /// Create new address validator
    pub fn new() -> Self {
        let mut secret = [0u8; 32];
        // Generate random secret
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        
        for (i, byte) in secret.iter_mut().enumerate() {
            *byte = ((seed >> (i * 8)) & 0xff) as u8;
        }

        Self {
            secret,
            tokens: Vec::new(),
            max_tokens: 1000,
        }
    }

    /// Issue a new token
    pub fn issue_token(&mut self, client_addr: SocketAddr) -> ValidationToken {
        // Clean expired tokens
        self.tokens.retain(|t| !t.is_expired());

        // Limit tokens
        while self.tokens.len() >= self.max_tokens {
            self.tokens.remove(0);
        }

        let token = ValidationToken::new(client_addr, &self.secret);
        self.tokens.push(token.clone());
        token
    }

    /// Validate a token
    pub fn validate_token(&self, token_data: &[u8], client_addr: &SocketAddr) -> bool {
        for token in &self.tokens {
            if token.data == token_data && token.validate(client_addr, &self.secret) {
                return true;
            }
        }
        false
    }

    /// Rotate secret
    pub fn rotate_secret(&mut self) {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        
        for (i, byte) in self.secret.iter_mut().enumerate() {
            *byte = ((seed >> (i * 8)) & 0xff) as u8;
        }

        // Clear old tokens
        self.tokens.clear();
    }
}

impl Default for AddressValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_validation() {
        let local: SocketAddr = "127.0.0.1:1234".parse().unwrap();
        let remote: SocketAddr = "127.0.0.1:5678".parse().unwrap();

        let mut path = Path::new(PathId(1), local, remote);
        assert_eq!(path.state, PathState::Unknown);

        path.start_validation();
        assert_eq!(path.state, PathState::Validating);
        assert!(path.challenge.is_some());

        let challenge = path.challenge.unwrap();
        assert!(path.handle_response(&challenge));
        assert_eq!(path.state, PathState::Validated);
    }

    #[test]
    fn test_path_manager() {
        let mut manager = PathManager::new();

        let local: SocketAddr = "127.0.0.1:1234".parse().unwrap();
        let remote: SocketAddr = "127.0.0.1:5678".parse().unwrap();

        let id = manager.add_path(local, remote).unwrap();
        assert!(manager.get_path(id).is_some());
        assert!(manager.active_path().is_some());

        let challenge = manager.start_validation(id).unwrap();
        manager.handle_response(&remote, challenge);

        let path = manager.get_path(id).unwrap();
        assert_eq!(path.state, PathState::Validated);
    }

    #[test]
    fn test_mtu_discovery() {
        let mut mtu = MtuDiscovery::new();
        assert_eq!(mtu.current_mtu(), 1200);

        mtu.start();
        assert_eq!(mtu.state(), MtuState::Searching);

        // Simulate successful probes
        mtu.probe_succeeded(1400);
        assert_eq!(mtu.current_mtu(), 1400);

        mtu.probe_failed(1500);
        // Should continue searching
    }

    #[test]
    fn test_address_validator() {
        let mut validator = AddressValidator::new();

        let addr: SocketAddr = "127.0.0.1:1234".parse().unwrap();
        let token = validator.issue_token(addr);

        assert!(validator.validate_token(&token.data, &addr));
        assert!(!validator.validate_token(&token.data, &"127.0.0.1:5678".parse().unwrap()));
    }
}
