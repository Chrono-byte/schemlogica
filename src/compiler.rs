use crate::semantics::Semantics;
use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize, Clone)]
pub struct Gate {
    pub id: String,
    pub kind: String,
    pub inputs: Vec<String>,
    pub output: String,
}

#[derive(Serialize)]
pub struct Circuit {
    pub gates: Vec<Gate>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

use std::sync::atomic::{AtomicUsize, Ordering};
static GID: AtomicUsize = AtomicUsize::new(1);
fn next_id() -> String {
    let id = GID.fetch_add(1, Ordering::SeqCst);
    format!("g{}", id)
}

pub fn compile(program: &Value, sem: &Semantics) -> Result<Circuit> {
    let mut gates = Vec::new();
    let mut var_signal = std::collections::HashMap::new();
    // Initialize variables as external input signals (levers). We'll represent each
    // declared variable as a named signal `sig_<var>`; the layout phase will place
    // a Lever/BUF primitive for these inputs if needed.
    for v in &sem.vars {
        var_signal.insert(v.clone(), format!("sig_{}", v));
    }

    fn compile_expr(
        expr: &Value,
        var_signal: &mut std::collections::HashMap<String, String>,
        gates: &mut Vec<Gate>,
    ) -> Result<String> {
        match expr.get("type").and_then(|t| t.as_str()) {
            Some("Literal") => {
                if let Some(b) = expr.get("value").and_then(|v| v.as_bool()) {
                    let sig = if b {
                        "CONST_TRUE".to_string()
                    } else {
                        "CONST_FALSE".to_string()
                    };
                    println!("schemlogica: compile_expr Literal -> {}", sig);
                    return Ok(sig);
                }
                anyhow::bail!("Only boolean literals allowed");
            }
            Some("Identifier") => {
                if let Some(name) = expr.get("name").and_then(|n| n.as_str()) {
                    if let Some(s) = var_signal.get(name) {
                        // If the identifier refers to a constant, return it directly.
                        if s == "CONST_TRUE" || s == "CONST_FALSE" {
                            return Ok(s.clone());
                        }
                        // Return the existing signal id directly to avoid emitting
                        // duplicate BUF chains for each reference. Routing/layout can
                        // decide whether an explicit buffer primitive is necessary.
                        return Ok(s.clone());
                    }
                    anyhow::bail!("Undefined identifier: {}", name);
                }
                anyhow::bail!("Malformed Identifier");
            }
            Some("UnaryExpression") => {
                if expr.get("operator").and_then(|o| o.as_str()) != Some("!") {
                    anyhow::bail!("Only ! supported");
                }
                let arg = expr.get("argument").expect("missing argument");
                let in_sig = compile_expr(arg, var_signal, gates)?;
                let out = next_id();
                let gid = next_id();
                println!(
                    "schemlogica: emit NOT gate id={} out={} in={}",
                    gid, out, in_sig
                );
                gates.push(Gate {
                    id: gid,
                    kind: "NOT".to_string(),
                    inputs: vec![in_sig.clone()],
                    output: out.clone(),
                });
                Ok(out)
            }
            Some("LogicalExpression") => {
                let left = expr.get("left").expect("left").clone();
                let right = expr.get("right").expect("right").clone();
                let lsig = compile_expr(&left, var_signal, gates)?;
                let rsig = compile_expr(&right, var_signal, gates)?;
                let op = expr.get("operator").and_then(|o| o.as_str()).unwrap_or("");
                let typ = match op {
                    "&&" => "AND",
                    "||" => "OR",
                    _ => anyhow::bail!("Only && and || supported"),
                };
                let out = next_id();
                let gid = next_id();
                println!(
                    "schemlogica: emit {} gate id={} out={} in1={} in2={}",
                    typ, gid, out, lsig, rsig
                );
                gates.push(Gate {
                    id: gid,
                    kind: typ.to_string(),
                    inputs: vec![lsig.clone(), rsig.clone()],
                    output: out.clone(),
                });
                Ok(out)
            }
            Some("ConditionalExpression") => {
                let cond = expr.get("test").expect("test");
                let cons = expr.get("consequent").expect("cons");
                let alt = expr.get("alternate").expect("alt");
                let c_sig = compile_expr(cond, var_signal, gates)?;
                let cons_sig = compile_expr(cons, var_signal, gates)?;
                let alt_sig = compile_expr(alt, var_signal, gates)?;
                let ca = next_id();
                let gid1 = next_id();
                println!(
                    "schemlogica: emit AND gate id={} out={} in1={} in2={}",
                    gid1, ca, c_sig, cons_sig
                );
                gates.push(Gate {
                    id: gid1,
                    kind: "AND".to_string(),
                    inputs: vec![c_sig.clone(), cons_sig.clone()],
                    output: ca.clone(),
                });
                let nc = next_id();
                let gid2 = next_id();
                println!(
                    "schemlogica: emit NOT gate id={} out={} in={}",
                    gid2, nc, c_sig
                );
                gates.push(Gate {
                    id: gid2,
                    kind: "NOT".to_string(),
                    inputs: vec![c_sig.clone()],
                    output: nc.clone(),
                });
                let nb = next_id();
                let gid3 = next_id();
                println!(
                    "schemlogica: emit AND gate id={} out={} in1={} in2={}",
                    gid3, nb, nc, alt_sig
                );
                gates.push(Gate {
                    id: gid3,
                    kind: "AND".to_string(),
                    inputs: vec![nc.clone(), alt_sig.clone()],
                    output: nb.clone(),
                });
                let out = next_id();
                let gid4 = next_id();
                println!(
                    "schemlogica: emit OR gate id={} out={} in1={} in2={}",
                    gid4, out, ca, nb
                );
                gates.push(Gate {
                    id: gid4,
                    kind: "OR".to_string(),
                    inputs: vec![ca.clone(), nb.clone()],
                    output: out.clone(),
                });
                Ok(out)
            }
            Some("BinaryExpression") => {
                let left = expr.get("left").expect("left").clone();
                let right = expr.get("right").expect("right").clone();
                let lsig = compile_expr(&left, var_signal, gates)?;
                let rsig = compile_expr(&right, var_signal, gates)?;

                // Expand XOR into primitive gates using only AND/OR/NOT:
                // xor = (lsig || rsig) && !(lsig && rsig)
                let or_sig = next_id();
                let gid_or = next_id();
                println!(
                    "schemlogica: emit OR gate id={} out={} in1={} in2={}",
                    gid_or, or_sig, lsig, rsig
                );
                gates.push(Gate {
                    id: gid_or.clone(),
                    kind: "OR".to_string(),
                    inputs: vec![lsig.clone(), rsig.clone()],
                    output: or_sig.clone(),
                });

                let and_sig = next_id();
                let gid_and = next_id();
                println!(
                    "schemlogica: emit AND gate id={} out={} in1={} in2={}",
                    gid_and, and_sig, lsig, rsig
                );
                gates.push(Gate {
                    id: gid_and.clone(),
                    kind: "AND".to_string(),
                    inputs: vec![lsig.clone(), rsig.clone()],
                    output: and_sig.clone(),
                });

                let not_and = next_id();
                let gid_not = next_id();
                println!(
                    "schemlogica: emit NOT gate id={} out={} in={}",
                    gid_not, not_and, and_sig
                );
                gates.push(Gate {
                    id: gid_not.clone(),
                    kind: "NOT".to_string(),
                    inputs: vec![and_sig.clone()],
                    output: not_and.clone(),
                });

                let xor = next_id();
                let gid_final_and = next_id();
                println!(
                    "schemlogica: emit AND gate id={} out={} in1={} in2={}",
                    gid_final_and, xor, or_sig, not_and
                );
                gates.push(Gate {
                    id: gid_final_and.clone(),
                    kind: "AND".to_string(),
                    inputs: vec![or_sig.clone(), not_and.clone()],
                    output: xor.clone(),
                });

                match expr.get("operator").and_then(|o| o.as_str()) {
                    Some("==") => {
                        let out = next_id();
                        let gid = next_id();
                        println!(
                            "schemlogica: emit NOT gate id={} out={} in={}",
                            gid, out, xor
                        );
                        gates.push(Gate {
                            id: gid,
                            kind: "NOT".to_string(),
                            inputs: vec![xor.clone()],
                            output: out.clone(),
                        });
                        Ok(out)
                    }
                    Some("!=") => Ok(xor),
                    _ => anyhow::bail!("Only == and != supported"),
                }
            }
            other => anyhow::bail!("Unsupported expression kind in compile: {:?}", other),
        }
    }

    let mut outputs = Vec::new();
    if let Some(body) = program.get("body").and_then(|b| b.as_array()) {
        for stmt in body {
            if let Some(t) = stmt.get("type").and_then(|s| s.as_str()) {
                if t == "VariableDeclaration" {
                    if let Some(decls) = stmt.get("declarations").and_then(|d| d.as_array()) {
                        for d in decls {
                            if let Some(id) = d.get("id") {
                                if id.get("type").and_then(|s| s.as_str()) == Some("Identifier") {
                                    let name =
                                        id.get("name").and_then(|n| n.as_str()).expect("name");
                                    if let Some(init) = d.get("init") {
                                        let sig = compile_expr(init, &mut var_signal, &mut gates)?;
                                        var_signal.insert(name.to_string(), sig);
                                    } else {
                                        anyhow::bail!(
                                            "Variable declarations must have initializers"
                                        );
                                    }
                                } else {
                                    anyhow::bail!("Destructuring not supported");
                                }
                            }
                        }
                    }
                } else if t == "ExpressionStatement" {
                    if let Some(expr) = stmt.get("expression") {
                        if expr.get("type").and_then(|s| s.as_str()) == Some("AssignmentExpression")
                        {
                            if let Some(left) = expr.get("left") {
                                if left.get("type").and_then(|s| s.as_str()) == Some("Identifier") {
                                    let name = left.get("name").and_then(|n| n.as_str()).unwrap();
                                    let right = expr.get("right").expect("right");
                                    let sig = compile_expr(right, &mut var_signal, &mut gates)?;
                                    // If the right-hand side is a constant, create a BUF so
                                    // the assignment produces a concrete signal and thus a
                                    // placed primitive. If compile_expr already emitted a
                                    // BUF (for identifiers), sig will be a fresh signal.
                                    if sig == "CONST_TRUE" || sig == "CONST_FALSE" {
                                        let out_sig = next_id();
                                        let gid = next_id();
                                        println!(
                                            "schemlogica: emit BUF gate id={} out={} in={}",
                                            gid, out_sig, sig
                                        );
                                        gates.push(Gate {
                                            id: gid,
                                            kind: "BUF".to_string(),
                                            inputs: vec![sig.clone()],
                                            output: out_sig.clone(),
                                        });
                                        var_signal.insert(name.to_string(), out_sig);
                                    } else {
                                        var_signal.insert(name.to_string(), sig);
                                    }
                                    outputs.push(name.to_string());
                                } else {
                                    anyhow::bail!("Only identifier assignments supported");
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Resolve outputs vector from variable names to the actual signal names produced
    let mut output_signals: Vec<String> = Vec::new();
    for name in outputs {
        if let Some(sig) = var_signal.get(&name) {
            output_signals.push(sig.clone());
        } else {
            // fallback: keep the variable name if no mapping found
            output_signals.push(name.clone());
        }
    }

    Ok(Circuit {
        gates,
        inputs: sem.vars.clone(),
        outputs: output_signals,
    })
}
