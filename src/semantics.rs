use anyhow::Result;
use serde_json::Value;

pub struct Semantics {
    pub vars: Vec<String>,
}

pub fn analyze(program: &Value) -> Result<Semantics> {
    let mut vars = Vec::new();
    if let Some(body) = program.get("body").and_then(|b| b.as_array()) {
        for stmt in body {
            if let Some(t) = stmt.get("type").and_then(|s| s.as_str()) {
                if t == "VariableDeclaration" {
                    // ensure kind == let
                    if let Some(kind) = stmt.get("kind").and_then(|k| k.as_str()) {
                        if kind != "let" {
                            anyhow::bail!("Only `let` declarations are allowed");
                        }
                    }
                    if let Some(decls) = stmt.get("declarations").and_then(|d| d.as_array()) {
                        for d in decls {
                            if let Some(id) = d.get("id") {
                                if id.get("type").and_then(|s| s.as_str()) == Some("Identifier") {
                                    if let Some(name) = id.get("name").and_then(|n| n.as_str()) {
                                        vars.push(name.to_string());
                                    }
                                } else {
                                    anyhow::bail!("Destructuring not supported");
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(Semantics { vars })
}
