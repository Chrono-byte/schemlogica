use crate::compiler::Circuit;
use crate::primitives::primitive_for;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};

// Layout constants
const GATE_SPACING_X: i32 = 12;  // Horizontal spacing between gates
const GATE_SPACING_Z: i32 = 16;  // Vertical spacing between gate rows (Increased for flat routing)
const LAYOUT_START_X: i32 = 0;
const LAYOUT_START_Y: i32 = 0;
const LAYOUT_START_Z: i32 = 0;

#[derive(Serialize)]
pub struct Layout {
    pub positions: Vec<(String, i32, i32, i32)>,
}

pub fn layout_circuit(circuit: &Circuit) -> Layout {
    let mut positions = Vec::new();
    
    if circuit.gates.is_empty() {
        return Layout { positions };
    }
    
    // Build dependency graph: gate_id -> list of gates that depend on it
    let mut depends_on: HashMap<String, Vec<String>> = HashMap::new();
    let mut dependents: HashMap<String, Vec<String>> = HashMap::new();
    
    for gate in &circuit.gates {
        depends_on.insert(gate.id.clone(), Vec::new());
        dependents.insert(gate.id.clone(), Vec::new());
    }
    
    // Map output signals to the gates that produce them
    let mut signal_to_gate: HashMap<String, String> = HashMap::new();
    for gate in &circuit.gates {
        signal_to_gate.insert(gate.output.clone(), gate.id.clone());
    }
    
    // Build dependency relationships
    for gate in &circuit.gates {
        for input_signal in &gate.inputs {
            if let Some(producer_id) = signal_to_gate.get(input_signal) {
                depends_on.get_mut(&gate.id).unwrap().push(producer_id.clone());
                dependents.get_mut(producer_id).unwrap().push(gate.id.clone());
            }
        }
    }
    
    // Topological sort to determine levels (depth in circuit)
    let mut levels: HashMap<String, usize> = HashMap::new();
    let mut in_degree: HashMap<String, usize> = HashMap::new();
    
    for gate in &circuit.gates {
        in_degree.insert(gate.id.clone(), depends_on[&gate.id].len());
    }
    
    let mut queue: VecDeque<String> = VecDeque::new();
    
    // Start with gates that have no dependencies (inputs, constants)
    for gate in &circuit.gates {
        if in_degree[&gate.id] == 0 {
            queue.push_back(gate.id.clone());
            levels.insert(gate.id.clone(), 0);
        }
    }
    
    // Process gates level by level
    while let Some(gate_id) = queue.pop_front() {
        let current_level = levels[&gate_id];
        
        for dependent_id in &dependents[&gate_id] {
            let degree = in_degree.get_mut(dependent_id).unwrap();
            *degree -= 1;
            
            // Update level to be max of all producer levels + 1
            let new_level = current_level + 1;
            levels.entry(dependent_id.clone())
                .and_modify(|l| *l = (*l).max(new_level))
                .or_insert(new_level);
            
            if *degree == 0 {
                queue.push_back(dependent_id.clone());
            }
        }
    }
    
    // Group gates by level
    let mut gates_by_level: HashMap<usize, Vec<String>> = HashMap::new();
    for gate in &circuit.gates {
        let level = *levels.get(&gate.id).unwrap_or(&0);
        gates_by_level.entry(level).or_insert_with(Vec::new).push(gate.id.clone());
    }
    
    // Place gates level by level
    let max_level = gates_by_level.keys().max().copied().unwrap_or(0);
    
    for level in 0..=max_level {
        if let Some(gate_ids) = gates_by_level.get(&level) {
            let mut current_x = LAYOUT_START_X;
            let z = LAYOUT_START_Z + (level as i32) * GATE_SPACING_Z;
            
            for gate_id in gate_ids {
                // Find the gate to get its kind
                if let Some(gate) = circuit.gates.iter().find(|g| g.id == *gate_id) {
                    let prim = primitive_for(&gate.kind);
                    
                    positions.push((gate_id.clone(), current_x, LAYOUT_START_Y, z));
                    
                    // Advance X by gate width plus spacing
                    current_x += prim.size_x + GATE_SPACING_X;
                }
            }
        }
    }
    
    Layout { positions }
}
