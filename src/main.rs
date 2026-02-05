use std::fs;
use std::path::Path;

mod compiler;
mod layout;
mod optimizer;
mod parser;
mod primitives;
mod schematic;
mod semantics;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} input.js out.litematic", args[0]);
        std::process::exit(2);
    }
    let in_path = &args[1];
    let out_path = &args[2];
    let code = fs::read_to_string(in_path)?;

    let program = parser::parse_and_validate(&code)?;
    // Debug: print parsed program JSON
    println!(
        "schemlogica: parsed program = {}",
        serde_json::to_string_pretty(&program)?
    );
    let sem = semantics::analyze(&program)?;
    let circuit = compiler::compile(&program, &sem)?;
    let circuit = optimizer::optimize(circuit);
    let layout = layout::layout_circuit(&circuit);
    schematic::write_schem(&circuit, &layout, Path::new(out_path))?;
    println!("Wrote litematic to {}", out_path);
    Ok(())
}
