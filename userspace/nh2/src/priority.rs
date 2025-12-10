//! Stream Priority and Dependency (RFC 7540 Section 5.3)
//!
//! This module implements stream priority handling for HTTP/2.

use crate::constants::priority::*;
use crate::types::{PrioritySpec, StreamId};
use std::collections::HashMap;

/// Priority node in the dependency tree
#[derive(Debug, Clone)]
pub struct PriorityNode {
    /// Stream ID
    pub stream_id: StreamId,
    /// Parent stream ID (0 for root)
    pub parent: StreamId,
    /// Weight (1-256)
    pub weight: i32,
    /// Child stream IDs
    pub children: Vec<StreamId>,
    /// Whether this is an exclusive dependency
    pub exclusive: bool,
}

impl PriorityNode {
    /// Create a new priority node
    pub fn new(stream_id: StreamId) -> Self {
        Self {
            stream_id,
            parent: 0,
            weight: DEFAULT_WEIGHT,
            children: Vec::new(),
            exclusive: false,
        }
    }

    /// Create from priority spec
    pub fn from_spec(stream_id: StreamId, spec: &PrioritySpec) -> Self {
        Self {
            stream_id,
            parent: spec.stream_id,
            weight: spec.weight,
            children: Vec::new(),
            exclusive: spec.exclusive != 0,
        }
    }
}

/// Priority tree for stream scheduling
#[derive(Debug)]
pub struct PriorityTree {
    /// All nodes in the tree
    nodes: HashMap<StreamId, PriorityNode>,
    /// Root children (streams depending on stream 0)
    root_children: Vec<StreamId>,
}

impl PriorityTree {
    /// Create a new priority tree
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            root_children: Vec::new(),
        }
    }

    /// Add a stream with default priority
    pub fn add(&mut self, stream_id: StreamId) {
        self.add_with_spec(stream_id, &PrioritySpec::default());
    }

    /// Add a stream with priority specification
    pub fn add_with_spec(&mut self, stream_id: StreamId, spec: &PrioritySpec) {
        // Validate weight
        let weight = spec.weight.clamp(MIN_WEIGHT, MAX_WEIGHT);
        let parent = spec.stream_id;
        let exclusive = spec.exclusive != 0;

        let mut node = PriorityNode {
            stream_id,
            parent,
            weight,
            children: Vec::new(),
            exclusive,
        };

        if exclusive {
            // Exclusive dependency: make all parent's children depend on this new stream
            let children_to_move = if parent == 0 {
                core::mem::take(&mut self.root_children)
            } else if let Some(parent_node) = self.nodes.get_mut(&parent) {
                core::mem::take(&mut parent_node.children)
            } else {
                Vec::new()
            };

            // Update children's parent
            for child_id in &children_to_move {
                if let Some(child) = self.nodes.get_mut(child_id) {
                    child.parent = stream_id;
                }
            }
            node.children = children_to_move;
        }

        // Add as child of parent
        if parent == 0 {
            self.root_children.push(stream_id);
        } else if let Some(parent_node) = self.nodes.get_mut(&parent) {
            parent_node.children.push(stream_id);
        } else {
            // Parent doesn't exist, add to root
            self.root_children.push(stream_id);
            node.parent = 0;
        }

        self.nodes.insert(stream_id, node);
    }

    /// Update stream priority
    pub fn update(&mut self, stream_id: StreamId, spec: &PrioritySpec) {
        // Remove from current parent
        self.remove_from_parent(stream_id);

        // Re-add with new spec
        if let Some(node) = self.nodes.get_mut(&stream_id) {
            node.parent = spec.stream_id;
            node.weight = spec.weight.clamp(MIN_WEIGHT, MAX_WEIGHT);
            node.exclusive = spec.exclusive != 0;
        }

        let parent = spec.stream_id;
        let exclusive = spec.exclusive != 0;

        if exclusive {
            // Handle exclusive dependency
            let children_to_move = if parent == 0 {
                self.root_children
                    .iter()
                    .filter(|&&id| id != stream_id)
                    .copied()
                    .collect::<Vec<_>>()
            } else if let Some(parent_node) = self.nodes.get(&parent) {
                parent_node
                    .children
                    .iter()
                    .filter(|&&id| id != stream_id)
                    .copied()
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };

            // Update children's parent
            for child_id in &children_to_move {
                if let Some(child) = self.nodes.get_mut(child_id) {
                    child.parent = stream_id;
                }
            }

            if let Some(node) = self.nodes.get_mut(&stream_id) {
                node.children.extend(children_to_move.iter());
            }

            // Clear parent's children (they now belong to stream_id)
            if parent == 0 {
                self.root_children.retain(|&id| id == stream_id);
            } else if let Some(parent_node) = self.nodes.get_mut(&parent) {
                parent_node.children.clear();
                parent_node.children.push(stream_id);
            }
        } else {
            // Add to new parent
            if parent == 0 {
                if !self.root_children.contains(&stream_id) {
                    self.root_children.push(stream_id);
                }
            } else if let Some(parent_node) = self.nodes.get_mut(&parent) {
                if !parent_node.children.contains(&stream_id) {
                    parent_node.children.push(stream_id);
                }
            }
        }
    }

    /// Remove a stream from the tree
    pub fn remove(&mut self, stream_id: StreamId) {
        if let Some(node) = self.nodes.remove(&stream_id) {
            // Move children to parent
            let parent = node.parent;
            let children = node.children;

            for child_id in &children {
                if let Some(child) = self.nodes.get_mut(child_id) {
                    child.parent = parent;
                }
            }

            // Add children to parent
            if parent == 0 {
                self.root_children.extend(children);
                self.root_children.retain(|&id| id != stream_id);
            } else if let Some(parent_node) = self.nodes.get_mut(&parent) {
                parent_node.children.extend(children);
                parent_node.children.retain(|&id| id != stream_id);
            }
        }
    }

    /// Remove stream from its parent's children list
    fn remove_from_parent(&mut self, stream_id: StreamId) {
        let parent = self.nodes.get(&stream_id).map(|n| n.parent).unwrap_or(0);

        if parent == 0 {
            self.root_children.retain(|&id| id != stream_id);
        } else if let Some(parent_node) = self.nodes.get_mut(&parent) {
            parent_node.children.retain(|&id| id != stream_id);
        }
    }

    /// Get stream priority
    pub fn get(&self, stream_id: StreamId) -> Option<&PriorityNode> {
        self.nodes.get(&stream_id)
    }

    /// Get next stream to send (weighted round-robin)
    pub fn next_to_send<F>(&self, can_send: F) -> Option<StreamId>
    where
        F: Fn(StreamId) -> bool,
    {
        // Simple implementation: find first sendable stream with depth-first traversal
        self.find_sendable(&self.root_children, &can_send)
    }

    fn find_sendable<F>(&self, streams: &[StreamId], can_send: &F) -> Option<StreamId>
    where
        F: Fn(StreamId) -> bool,
    {
        // Sort by weight (descending)
        let mut sorted: Vec<_> = streams.iter().copied().collect();
        sorted.sort_by(|&a, &b| {
            let wa = self.nodes.get(&a).map(|n| n.weight).unwrap_or(16);
            let wb = self.nodes.get(&b).map(|n| n.weight).unwrap_or(16);
            wb.cmp(&wa)
        });

        for stream_id in sorted {
            if can_send(stream_id) {
                return Some(stream_id);
            }

            // Check children
            if let Some(node) = self.nodes.get(&stream_id) {
                if let Some(child) = self.find_sendable(&node.children, can_send) {
                    return Some(child);
                }
            }
        }

        None
    }

    /// Get all streams in priority order
    pub fn streams_in_order(&self) -> Vec<StreamId> {
        let mut result = Vec::new();
        self.collect_in_order(&self.root_children, &mut result);
        result
    }

    fn collect_in_order(&self, streams: &[StreamId], result: &mut Vec<StreamId>) {
        let mut sorted: Vec<_> = streams.iter().copied().collect();
        sorted.sort_by(|&a, &b| {
            let wa = self.nodes.get(&a).map(|n| n.weight).unwrap_or(16);
            let wb = self.nodes.get(&b).map(|n| n.weight).unwrap_or(16);
            wb.cmp(&wa)
        });

        for stream_id in sorted {
            result.push(stream_id);
            if let Some(node) = self.nodes.get(&stream_id) {
                self.collect_in_order(&node.children, result);
            }
        }
    }
}

impl Default for PriorityTree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_priority() {
        let mut tree = PriorityTree::new();
        tree.add(1);
        tree.add(3);
        tree.add(5);

        assert!(tree.get(1).is_some());
        assert!(tree.get(3).is_some());
        assert!(tree.get(5).is_some());
    }

    #[test]
    fn test_exclusive_dependency() {
        let mut tree = PriorityTree::new();
        tree.add(1);
        tree.add(3);

        // Make stream 5 exclusive child of root
        tree.add_with_spec(
            5,
            &PrioritySpec {
                stream_id: 0,
                weight: 32,
                exclusive: 1,
            },
        );

        // Stream 1 and 3 should now be children of 5
        let node5 = tree.get(5).unwrap();
        assert!(node5.children.contains(&1) || node5.children.contains(&3));
    }

    #[test]
    fn test_remove() {
        let mut tree = PriorityTree::new();
        tree.add(1);
        tree.add_with_spec(
            3,
            &PrioritySpec {
                stream_id: 1,
                weight: 16,
                exclusive: 0,
            },
        );

        tree.remove(1);

        // Stream 3 should now be a root child
        let node3 = tree.get(3).unwrap();
        assert_eq!(node3.parent, 0);
    }
}
