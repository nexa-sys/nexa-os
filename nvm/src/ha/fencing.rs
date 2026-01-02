//! Node Fencing (STONITH)

use super::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Fencing manager
pub struct FencingManager {
    /// Fencing agents
    agents: HashMap<NodeId, FencingAgent>,
    /// Default method
    default_method: FencingMethod,
    /// Fencing history
    history: Vec<FencingEvent>,
}

impl FencingManager {
    pub fn new(default_method: FencingMethod) -> Self {
        Self {
            agents: HashMap::new(),
            default_method,
            history: Vec::new(),
        }
    }

    /// Register fencing agent for a node
    pub fn register(&mut self, node_id: NodeId, agent: FencingAgent) {
        self.agents.insert(node_id, agent);
    }

    /// Fence a node (STONITH - Shoot The Other Node In The Head)
    pub fn fence(&mut self, node_id: &NodeId, reason: &str) -> Result<(), HaError> {
        let agent = self.agents.get(node_id)
            .ok_or_else(|| HaError::NodeNotFound(node_id.clone()))?;

        let result = agent.execute_fence();
        
        let event = FencingEvent {
            timestamp: now(),
            node_id: node_id.clone(),
            method: agent.method,
            reason: reason.to_string(),
            success: result.is_ok(),
            error: result.as_ref().err().map(|e| e.to_string()),
        };
        
        self.history.push(event);
        
        result.map_err(|e| HaError::FencingFailed(e.to_string()))
    }

    /// Get fencing history
    pub fn history(&self) -> &[FencingEvent] {
        &self.history
    }

    /// Test fencing configuration
    pub fn test(&self, node_id: &NodeId) -> Result<(), HaError> {
        let agent = self.agents.get(node_id)
            .ok_or_else(|| HaError::NodeNotFound(node_id.clone()))?;
        
        agent.test()
    }
}

/// Fencing agent
#[derive(Debug, Clone)]
pub struct FencingAgent {
    /// Fencing method
    pub method: FencingMethod,
    /// Agent configuration
    pub config: FencingConfig,
}

/// Fencing configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FencingConfig {
    /// IPMI address
    pub ipmi_address: Option<String>,
    /// IPMI username
    pub ipmi_username: Option<String>,
    /// IPMI password
    pub ipmi_password: Option<String>,
    /// PDU address
    pub pdu_address: Option<String>,
    /// PDU outlet
    pub pdu_outlet: Option<u32>,
    /// SSH host
    pub ssh_host: Option<String>,
    /// SSH user
    pub ssh_user: Option<String>,
    /// SSH key path
    pub ssh_key: Option<String>,
    /// Custom command
    pub custom_command: Option<String>,
    /// Timeout (seconds)
    pub timeout: u32,
    /// Delay before fence (seconds)
    pub delay: u32,
}

impl Default for FencingConfig {
    fn default() -> Self {
        Self {
            ipmi_address: None,
            ipmi_username: None,
            ipmi_password: None,
            pdu_address: None,
            pdu_outlet: None,
            ssh_host: None,
            ssh_user: None,
            ssh_key: None,
            custom_command: None,
            timeout: 60,
            delay: 0,
        }
    }
}

impl FencingAgent {
    pub fn new(method: FencingMethod, config: FencingConfig) -> Self {
        Self { method, config }
    }

    /// Execute fence operation
    pub fn execute_fence(&self) -> Result<(), String> {
        match self.method {
            FencingMethod::Ipmi => self.fence_ipmi(),
            FencingMethod::Pdu => self.fence_pdu(),
            FencingMethod::Ssh => self.fence_ssh(),
            FencingMethod::Sbd => self.fence_sbd(),
            FencingMethod::Custom => self.fence_custom(),
            FencingMethod::None => Ok(()),
        }
    }

    /// Test fencing configuration
    pub fn test(&self) -> Result<(), HaError> {
        match self.method {
            FencingMethod::Ipmi => {
                if self.config.ipmi_address.is_none() {
                    return Err(HaError::FencingFailed("IPMI address not configured".into()));
                }
                // Would test IPMI connection
                Ok(())
            }
            FencingMethod::Pdu => {
                if self.config.pdu_address.is_none() {
                    return Err(HaError::FencingFailed("PDU address not configured".into()));
                }
                Ok(())
            }
            FencingMethod::Ssh => {
                if self.config.ssh_host.is_none() {
                    return Err(HaError::FencingFailed("SSH host not configured".into()));
                }
                Ok(())
            }
            FencingMethod::Custom => {
                if self.config.custom_command.is_none() {
                    return Err(HaError::FencingFailed("Custom command not configured".into()));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn fence_ipmi(&self) -> Result<(), String> {
        let addr = self.config.ipmi_address.as_ref()
            .ok_or("IPMI address not configured")?;
        let user = self.config.ipmi_username.as_ref()
            .ok_or("IPMI username not configured")?;
        let pass = self.config.ipmi_password.as_ref()
            .ok_or("IPMI password not configured")?;

        // In production: use ipmitool
        // ipmitool -I lanplus -H {addr} -U {user} -P {pass} chassis power off
        let _ = (addr, user, pass);
        Ok(())
    }

    fn fence_pdu(&self) -> Result<(), String> {
        let addr = self.config.pdu_address.as_ref()
            .ok_or("PDU address not configured")?;
        let outlet = self.config.pdu_outlet
            .ok_or("PDU outlet not configured")?;

        // In production: use SNMP or PDU-specific protocol
        let _ = (addr, outlet);
        Ok(())
    }

    fn fence_ssh(&self) -> Result<(), String> {
        let host = self.config.ssh_host.as_ref()
            .ok_or("SSH host not configured")?;

        // In production: SSH to node and run poweroff/reboot
        let _ = host;
        Ok(())
    }

    fn fence_sbd(&self) -> Result<(), String> {
        // Storage-based death (shared block device)
        // Write poison pill to SBD device
        Ok(())
    }

    fn fence_custom(&self) -> Result<(), String> {
        let cmd = self.config.custom_command.as_ref()
            .ok_or("Custom command not configured")?;

        // In production: execute custom script
        let _ = cmd;
        Ok(())
    }
}

/// Fencing event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FencingEvent {
    pub timestamp: u64,
    pub node_id: NodeId,
    pub method: FencingMethod,
    pub reason: String,
    pub success: bool,
    pub error: Option<String>,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl Default for FencingManager {
    fn default() -> Self {
        Self::new(FencingMethod::None)
    }
}
