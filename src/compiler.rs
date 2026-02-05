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

pub fn compile(program: &Value, _sem: &Semantics) -> Result<Circuit> {
    let mut gates = Vec::new();
    let mut var_signal = std::collections::HashMap::new();

    // Inject Constant Gates
    gates.push(Gate {
        id: "g_const_true".into(),
        kind: "CONST_TRUE".into(),
        inputs: vec![],
        output: "CONST_TRUE_SIG".into(),
    });
    gates.push(Gate {
        id: "g_const_false".into(),
        kind: "CONST_FALSE".into(),
        inputs: vec![],
        output: "CONST_FALSE_SIG".into(),
    });

    fn compile_expr(
        expr: &Value,
        var_signal: &mut std::collections::HashMap<String, String>,
        gates: &mut Vec<Gate>,
    ) -> Result<String> {
        match expr.get("type").and_then(|t| t.as_str()) {
            Some("Literal") => {
                if let Some(b) = expr.get("value").and_then(|v| v.as_bool()) {
                    Ok(if b {
                        "CONST_TRUE_SIG".into()
                    } else {
                        "CONST_FALSE_SIG".into()
                    })
                } else {
                    anyhow::bail!("Only boolean literals allowed")
                }
            }
            Some("Identifier") => {
                let name = expr.get("name").and_then(|n| n.as_str()).expect("name");
                if let Some(s) = var_signal.get(name) {
                    Ok(s.clone())
                } else {
                    anyhow::bail!("Undefined: {}", name)
                }
            }
            Some("UnaryExpression") => {
                let arg = compile_expr(expr.get("argument").unwrap(), var_signal, gates)?;
                let out = next_id();
                gates.push(Gate {
                    id: next_id(),
                    kind: "NOT".into(),
                    inputs: vec![arg],
                    output: out.clone(),
                });
                Ok(out)
            }
            Some("LogicalExpression") => {
                let l = compile_expr(expr.get("left").unwrap(), var_signal, gates)?;
                let r = compile_expr(expr.get("right").unwrap(), var_signal, gates)?;
                let op = expr.get("operator").and_then(|s| s.as_str()).unwrap();
                let kind = match op {
                    "&&" => "AND",
                    "||" => "OR",
                    _ => anyhow::bail!("Unsupported op"),
                };
                let out = next_id();
                gates.push(Gate {
                    id: next_id(),
                    kind: kind.into(),
                    inputs: vec![l, r],
                    output: out.clone(),
                });
                Ok(out)
            }
            Some("ConditionalExpression") => {
                let t = compile_expr(expr.get("test").unwrap(), var_signal, gates)?;
                let c = compile_expr(expr.get("consequent").unwrap(), var_signal, gates)?;
                let a = compile_expr(expr.get("alternate").unwrap(), var_signal, gates)?;
                // MUX: (t && c) || (!t && a)
                let not_t = next_id();
                gates.push(Gate {
                    id: next_id(),
                    kind: "NOT".into(),
                    inputs: vec![t.clone()],
                    output: not_t.clone(),
                });
                let t_and_c = next_id();
                gates.push(Gate {
                    id: next_id(),
                    kind: "AND".into(),
                    inputs: vec![t, c],
                    output: t_and_c.clone(),
                });
                let nt_and_a = next_id();
                gates.push(Gate {
                    id: next_id(),
                    kind: "AND".into(),
                    inputs: vec![not_t, a],
                    output: nt_and_a.clone(),
                });
                let out = next_id();
                gates.push(Gate {
                    id: next_id(),
                    kind: "OR".into(),
                    inputs: vec![t_and_c, nt_and_a],
                    output: out.clone(),
                });
                Ok(out)
            }
            Some("BinaryExpression") => {
                // Equality: (A == B) -> XNOR -> NOT (XOR)
                let l = compile_expr(expr.get("left").unwrap(), var_signal, gates)?;
                let r = compile_expr(expr.get("right").unwrap(), var_signal, gates)?;
                // XOR = (A || B) && !(A && B)
                let or_sig = next_id();
                gates.push(Gate {
                    id: next_id(),
                    kind: "OR".into(),
                    inputs: vec![l.clone(), r.clone()],
                    output: or_sig.clone(),
                });
                let and_sig = next_id();
                gates.push(Gate {
                    id: next_id(),
                    kind: "AND".into(),
                    inputs: vec![l, r],
                    output: and_sig.clone(),
                });
                let not_and = next_id();
                gates.push(Gate {
                    id: next_id(),
                    kind: "NOT".into(),
                    inputs: vec![and_sig],
                    output: not_and.clone(),
                });
                let xor = next_id();
                gates.push(Gate {
                    id: next_id(),
                    kind: "AND".into(),
                    inputs: vec![or_sig, not_and],
                    output: xor.clone(),
                });

                match expr.get("operator").and_then(|s| s.as_str()) {
                    Some("!=") => Ok(xor),
                    Some("==") => {
                        let out = next_id();
                        gates.push(Gate {
                            id: next_id(),
                            kind: "NOT".into(),
                            inputs: vec![xor],
                            output: out.clone(),
                        });
                        Ok(out)
                    }
                    _ => anyhow::bail!("Unsupported binary op"),
                }
            }
            _ => anyhow::bail!("Unsupported expr"),
        }
    }

    let mut declared_inputs = Vec::new();
    let mut outputs = Vec::new();

    if let Some(body) = program.get("body").and_then(|b| b.as_array()) {
        for stmt in body {
            if let Some(t) = stmt.get("type").and_then(|s| s.as_str()) {
                if t == "VariableDeclaration" {
                    for d in stmt.get("declarations").unwrap().as_array().unwrap() {
                        let name = d.get("id").unwrap().get("name").unwrap().as_str().unwrap();
                        if let Some(init) = d.get("init") {
                            let sig = compile_expr(init, &mut var_signal, &mut gates)?;
                            var_signal.insert(name.into(), sig);
                        } else {
                            // Uninitialized = INPUT lever
                            let out = format!("sig_{}", name);
                            gates.push(Gate {
                                id: next_id(),
                                kind: "INPUT".into(),
                                inputs: vec![],
                                output: out.clone(),
                            });
                            var_signal.insert(name.into(), out);
                            declared_inputs.push(name.into());
                        }
                    }
                } else if t == "ExpressionStatement" {
                    if let Some(expr) = stmt.get("expression") {
                        if expr.get("type").and_then(|s| s.as_str()) == Some("AssignmentExpression")
                        {
                            let name = expr
                                .get("left")
                                .unwrap()
                                .get("name")
                                .unwrap()
                                .as_str()
                                .unwrap();
                            let right = expr.get("right").unwrap();
                            let sig = compile_expr(right, &mut var_signal, &mut gates)?;
                            // Add Buffer to force a distinct signal output for assignment
                            let out = next_id();
                            gates.push(Gate {
                                id: next_id(),
                                kind: "BUF".into(),
                                inputs: vec![sig],
                                output: out.clone(),
                            });
                            var_signal.insert(name.into(), out.clone());
                            outputs.push(out);
                        }
                    }
                }
            }
        }
    }

    Ok(Circuit {
        gates,
        inputs: declared_inputs,
        outputs,
    })
}
