// daw_modules/src/engine/automation.rs

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationNode<T> {
    pub time: u64, // Position in samples
    pub value: T,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AutomationCurve<T> {
    nodes: Vec<AutomationNode<T>>,
}

impl<T> AutomationCurve<T> {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn nodes(&self) -> &[AutomationNode<T>] {
        &self.nodes
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
    }
}

// We specifically implement the math/interpolation for f32 (Gain, Pan, etc.)
// This keeps the engine pure and lock-free during real-time evaluation.
impl AutomationCurve<f32> {
    /// Inserts a node. If a node at the exact sample time exists, it overwrites it.
    /// Uses binary_search_by_key to guarantee O(log n) sorted insertion.
    pub fn insert_node(&mut self, time: u64, value: f32) {
        let node = AutomationNode { time, value };
        match self.nodes.binary_search_by_key(&time, |n| n.time) {
            Ok(pos) => self.nodes[pos].value = value, // Exact time match, overwrite
            Err(pos) => self.nodes.insert(pos, node), // Insert in sorted position
        }
    }

    /// Removes a node at a specific time, returning true if found and removed.
    pub fn remove_node_at_time(&mut self, time: u64) -> bool {
        if let Ok(pos) = self.nodes.binary_search_by_key(&time, |n| n.time) {
            self.nodes.remove(pos);
            true
        } else {
            false
        }
    }

    /// Pure math evaluation. Returns the exact interpolated value at a given sample position.
    pub fn get_value_at_time(&self, time: u64, default_value: f32) -> f32 {
        if self.nodes.is_empty() {
            return default_value;
        }

        let first = &self.nodes[0];
        if time <= first.time {
            return first.value;
        }

        let last = &self.nodes[self.nodes.len() - 1];
        if time >= last.time {
            return last.value;
        }

        // We are somewhere between the first and last node.
        match self.nodes.binary_search_by_key(&time, |n| n.time) {
            Ok(pos) => self.nodes[pos].value, // Exact hit on a node
            Err(pos) => {
                // pos is the insertion index, meaning:
                // pos - 1 is the previous node
                // pos is the next node
                let prev = &self.nodes[pos - 1];
                let next = &self.nodes[pos];

                let range = (next.time - prev.time) as f64; // Use f64 to prevent overflow in division
                let progress = (time - prev.time) as f64 / range;

                // Linear interpolation: v1 + (v2 - v1) * progress
                prev.value + ((next.value - prev.value) * progress as f32)
            }
        }
    }
}