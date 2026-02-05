use anyhow::Result;
use serde_json::{json, Value};

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::SourceType;
use oxc_syntax::operator::{AssignmentOperator, BinaryOperator, LogicalOperator, UnaryOperator};

fn id_name_from_binding(ident: &BindingIdentifier) -> String {
    ident.name.as_str().to_string()
}

fn id_name_from_identref(ident: &IdentifierReference) -> String {
    ident.name.as_str().to_string()
}

fn expr_to_json<'a>(expr: &Expression<'a>) -> anyhow::Result<Value> {
    match expr {
        Expression::BooleanLiteral(boxed) => {
            let lit = &**boxed;
            Ok(json!({"type":"Literal","value": lit.value }))
        }
        Expression::Identifier(boxed) => {
            let id = &**boxed;
            Ok(json!({"type":"Identifier","name": id.name.as_str()}))
        }
        Expression::UnaryExpression(boxed) => {
            let u = &**boxed;
            if u.operator != UnaryOperator::LogicalNot {
                anyhow::bail!("Only ! unary operator supported");
            }
            let arg = expr_to_json(&u.argument)?;
            Ok(json!({"type":"UnaryExpression","operator":"!","argument": arg}))
        }
        Expression::LogicalExpression(boxed) => {
            let le = &**boxed;
            let op = match le.operator {
                LogicalOperator::And => "&&",
                LogicalOperator::Or => "||",
                _ => anyhow::bail!("Unsupported logical operator"),
            };
            let left = expr_to_json(&le.left)?;
            let right = expr_to_json(&le.right)?;
            Ok(json!({"type":"LogicalExpression","operator": op, "left": left, "right": right}))
        }
        Expression::BinaryExpression(boxed) => {
            let be = &**boxed;
            let op = match be.operator {
                BinaryOperator::Equality => "==",
                BinaryOperator::Inequality => "!=",
                _ => anyhow::bail!("Only == and != supported in binary expressions"),
            };
            let left = expr_to_json(&be.left)?;
            let right = expr_to_json(&be.right)?;
            Ok(json!({"type":"BinaryExpression","operator": op, "left": left, "right": right}))
        }
        Expression::ConditionalExpression(boxed) => {
            let ce = &**boxed;
            let test = expr_to_json(&ce.test)?;
            let cons = expr_to_json(&ce.consequent)?;
            let alt = expr_to_json(&ce.alternate)?;
            Ok(
                json!({"type":"ConditionalExpression","test": test, "consequent": cons, "alternate": alt}),
            )
        }
        Expression::ParenthesizedExpression(boxed) => {
            let pe = &**boxed;
            expr_to_json(&pe.expression)
        }
        Expression::AssignmentExpression(boxed) => {
            let ae = &**boxed;
            if ae.operator != AssignmentOperator::Assign {
                anyhow::bail!("Only = assignment supported");
            }
            match &ae.left {
                AssignmentTarget::AssignmentTargetIdentifier(id_box) => {
                    let id = &**id_box;
                    let name = id.name.as_str().to_string();
                    let right = expr_to_json(&ae.right)?;
                    Ok(
                        json!({"type":"AssignmentExpression","operator":"=","left": {"type":"Identifier","name": name}, "right": right}),
                    )
                }
                _ => anyhow::bail!("Only identifier assignment targets supported"),
            }
        }
        _ => anyhow::bail!("Unsupported expression node"),
    }
}

fn stmt_to_json<'a>(stmt: &Statement<'a>) -> anyhow::Result<Value> {
    use Statement::*;
    match stmt {
        VariableDeclaration(vd) => {
            if vd.kind != VariableDeclarationKind::Let {
                anyhow::bail!("Only `let` declarations are allowed");
            }
            let mut decls = Vec::new();
            for d in &vd.declarations {
                match &d.id {
                    BindingPattern::BindingIdentifier(bi) => {
                        let name = bi.name.as_str().to_string();
                        if let Some(init_expr) = &d.init {
                            let init = expr_to_json(init_expr)?;
                            decls.push(json!({"type":"VariableDeclarator","id": {"type":"Identifier","name": name}, "init": init}));
                        } else {
                            // Allow uninitialized variables: treat them as inputs/levers later in the pipeline.
                            decls.push(json!({"type":"VariableDeclarator","id": {"type":"Identifier","name": name}}));
                        }
                    }
                    _ => anyhow::bail!("Destructuring not supported"),
                }
            }
            Ok(json!({"type":"VariableDeclaration","kind":"let","declarations": decls}))
        }
        ExpressionStatement(es) => {
            let expr = expr_to_json(&es.expression)?;
            Ok(json!({"type":"ExpressionStatement","expression": expr}))
        }
        _ => anyhow::bail!("Unsupported top-level statement: {:?}", stmt),
    }
}

pub fn parse_and_validate(code: &str) -> Result<Value> {
    let alloc = Allocator::default();
    let parser = Parser::new(&alloc, code, SourceType::default());
    let ret = parser.parse();
    if ret.panicked || !ret.errors.is_empty() {
        anyhow::bail!("Parse errors: {:?}", ret.errors);
    }

    let mut body = Vec::new();
    for stmt in &ret.program.body {
        let s = stmt_to_json(stmt)?;
        body.push(s);
    }

    Ok(json!({"type":"Program","body": body}))
}
