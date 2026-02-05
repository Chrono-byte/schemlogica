use crate::compiler::Circuit;

// Simple optimizer: dead gate elimination & identity simplifications & const folding
pub fn optimize(mut circuit: Circuit) -> Circuit {
    // dead-gate elimination: find gates reachable from outputs
    let mut producers = std::collections::HashMap::new();
    for g in &circuit.gates {
        producers.insert(g.output.clone(), g.id.clone());
    }
    let mut reachable = std::collections::HashSet::new();
    // Start reachability from the output signals directly (compile now resolves outputs to signals)
    let mut stack: Vec<String> = circuit.outputs.clone();
    while let Some(sig) = stack.pop() {
        if reachable.contains(&sig) {
            continue;
        }
        reachable.insert(sig.clone());
        if let Some(gid) = producers.get(&sig) {
            if let Some(g) = circuit.gates.iter().find(|gg| &gg.id == gid) {
                for inp in &g.inputs {
                    stack.push(inp.clone());
                }
            }
        }
    }
    circuit.gates.retain(|g| reachable.contains(&g.output));
    // TODO: more optimizations
    circuit
}
