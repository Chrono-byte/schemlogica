use crate::compiler::Circuit;
use serde::Serialize;

#[derive(Serialize)]
pub struct Layout {
    pub positions: Vec<(String, i32, i32, i32)>,
}

pub fn layout_circuit(circuit: &Circuit) -> Layout {
    let mut positions = Vec::new();
    // Simpler, safer layout for v1: place gates in a single long line with
    // generous spacing to avoid collisions and make wiring straightforward.
    let mut current_x = 0i32;
    for g in &circuit.gates {
        positions.push((g.id.clone(), current_x, 0, 0));
        // give a wide spacing to avoid z-overlap and allow room for routing
        current_x += 10;
    }
    Layout { positions }
}
