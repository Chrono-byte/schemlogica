# schemlogica — Boolean-only JS → Redstone MVP

This project contains a minimal end-to-end pipeline that:

- Parses a restricted boolean-only JavaScript subset using `oxc-parser`.
- Performs a semantic check (boolean-only, `let` declarations, ternary, &&, ||, !).
- Compiles expressions to a gate-based `Circuit` (AND/OR/NOT/MUX).
- Lays out gates into 3D coordinates (simple layering by dependency depth).
- Emits a JSON "schematic" describing blocks/ports. This is a lightweight MVP format
  (not a full Minecraft `.schem` NBT file) so you can inspect and iterate quickly.

Run:

1. npm install
2. node src/cli.js input.js out_dir

Outputs written to `out_dir/circuit.json` and `out_dir/schematic.json`.
