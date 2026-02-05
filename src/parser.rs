use anyhow::Result;
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::SourceType;
use oxc_syntax::operator::{AssignmentOperator, BinaryOperator, LogicalOperator, UnaryOperator};
use serde_json::{json, Value};

fn expr_to_json<'a>(expr: &Expression<'a>) -> anyhow::Result<Value> {
    match expr {
        Expression::BooleanLiteral(boxed) => Ok(json!({"type":"Literal","value": boxed.value })),
        Expression::Identifier(boxed) => {
            Ok(json!({"type":"Identifier","name": boxed.name.as_str()}))
        }
        Expression::UnaryExpression(boxed) => {
            if boxed.operator != UnaryOperator::LogicalNot {
                anyhow::bail!("Only ! unary operator supported");
            }
            let arg = expr_to_json(&boxed.argument)?;
            Ok(json!({"type":"UnaryExpression","operator":"!","argument": arg}))
        }
        Expression::LogicalExpression(boxed) => {
            let op = match boxed.operator {
                LogicalOperator::And => "&&",
                LogicalOperator::Or => "||",
                _ => anyhow::bail!("Unsupported logical operator"),
            };
            let left = expr_to_json(&boxed.left)?;
            let right = expr_to_json(&boxed.right)?;
            Ok(json!({"type":"LogicalExpression","operator": op, "left": left, "right": right}))
        }
        Expression::BinaryExpression(boxed) => {
            let op = match boxed.operator {
                BinaryOperator::Equality => "==",
                BinaryOperator::Inequality => "!=",
                _ => anyhow::bail!("Only == and != supported in binary expressions"),
            };
            let left = expr_to_json(&boxed.left)?;
            let right = expr_to_json(&boxed.right)?;
            Ok(json!({"type":"BinaryExpression","operator": op, "left": left, "right": right}))
        }
        Expression::ConditionalExpression(boxed) => {
            let test = expr_to_json(&boxed.test)?;
            let cons = expr_to_json(&boxed.consequent)?;
            let alt = expr_to_json(&boxed.alternate)?;
            Ok(
                json!({"type":"ConditionalExpression","test": test, "consequent": cons, "alternate": alt}),
            )
        }
        Expression::ParenthesizedExpression(boxed) => expr_to_json(&boxed.expression),
        Expression::AssignmentExpression(boxed) => {
            if boxed.operator != AssignmentOperator::Assign {
                anyhow::bail!("Only = assignment supported");
            }
            match &boxed.left {
                AssignmentTarget::AssignmentTargetIdentifier(id_box) => {
                    let name = id_box.name.as_str().to_string();
                    let right = expr_to_json(&boxed.right)?;
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
    match stmt {
        Statement::VariableDeclaration(vd) => {
            if vd.kind != VariableDeclarationKind::Let {
                anyhow::bail!("Only `let` declarations are allowed");
            }
            let mut decls = Vec::new();
            for d in &vd.declarations {
                if let BindingPattern::BindingIdentifier(bi) = &d.id {
                    let name = bi.name.as_str().to_string();
                    if let Some(init_expr) = &d.init {
                        let init = expr_to_json(init_expr)?;
                        decls.push(json!({"type":"VariableDeclarator","id": {"type":"Identifier","name": name}, "init": init}));
                    } else {
                        decls.push(json!({"type":"VariableDeclarator","id": {"type":"Identifier","name": name}}));
                    }
                } else {
                    anyhow::bail!("Destructuring not supported");
                }
            }
            Ok(json!({"type":"VariableDeclaration","kind":"let","declarations": decls}))
        }
        Statement::ExpressionStatement(es) => {
            let expr = expr_to_json(&es.expression)?;
            Ok(json!({"type":"ExpressionStatement","expression": expr}))
        }
        _ => anyhow::bail!("Unsupported top-level statement"),
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
        body.push(stmt_to_json(stmt)?);
    }
    Ok(json!({"type":"Program","body": body}))
}
