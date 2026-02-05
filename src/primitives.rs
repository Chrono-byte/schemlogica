use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct BlockPlaque {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub name: String,
    pub properties: Option<Vec<(String, String)>>,
}

#[derive(Serialize)]
pub struct Primitive {
    pub name: String,
    pub size_x: i32,
    pub size_y: i32,
    pub size_z: i32,
    pub blocks: Vec<BlockPlaque>,
    /// Inputs are relative to origin (0,0,0)
    pub input_ports: Vec<(i32, i32, i32)>,
    /// Output is relative to origin (0,0,0)
    pub output_port: (i32, i32, i32),
}

// --- Helper Functions ---

fn make_block(x: i32, y: i32, z: i32, name: &str, props: Option<Vec<(&str, &str)>>) -> BlockPlaque {
    BlockPlaque {
        x,
        y,
        z,
        name: name.into(),
        properties: props.map(|v| v.into_iter().map(|(k, v)| (k.into(), v.into())).collect()),
    }
}

/// Creates a solid sandstone floor for the primitive
fn make_floor(blocks: &mut Vec<BlockPlaque>, size_x: i32, size_z: i32) {
    for x in 0..size_x {
        for z in 0..size_z {
            blocks.push(make_block(x, 0, z, "minecraft:sandstone", None));
        }
    }
}

// --- Gate Implementations ---

pub fn primitive_for(kind: &str) -> Primitive {
    let mut blocks = Vec::new();

    match kind {
        "CONST_TRUE" => {
            // A redstone block that always outputs power
            let (sx, sy, sz) = (1, 2, 1);
            make_floor(&mut blocks, sx, sz);
            blocks.push(make_block(0, 1, 0, "minecraft:redstone_block", None));
            Primitive {
                name: "CONST_TRUE".into(),
                size_x: sx,
                size_y: sy,
                size_z: sz,
                blocks,
                input_ports: vec![],
                output_port: (0, 1, 0),
            }
        }
        "CONST_FALSE" => {
            // A glass block (or air) that outputs nothing
            let (sx, sy, sz) = (1, 2, 1);
            make_floor(&mut blocks, sx, sz);
            blocks.push(make_block(0, 1, 0, "minecraft:glass", None));
            Primitive {
                name: "CONST_FALSE".into(),
                size_x: sx,
                size_y: sy,
                size_z: sz,
                blocks,
                input_ports: vec![],
                output_port: (0, 1, 0),
            }
        }
        "INPUT" => {
            // A lever on a block
            let (sx, sy, sz) = (1, 2, 1);
            make_floor(&mut blocks, sx, sz);
            blocks.push(make_block(0, 1, 0, "minecraft:cobblestone", None));
            blocks.push(make_block(
                0,
                2,
                0,
                "minecraft:lever",
                Some(vec![("face", "floor"), ("powered", "false")]),
            ));
            Primitive {
                name: "INPUT".into(),
                size_x: sx,
                size_y: sy,
                size_z: sz,
                blocks,
                input_ports: vec![],
                output_port: (0, 1, 0), // Current travels through the block
            }
        }
        "BUF" => {
            // Description: 2-tick Repeater
            // Size: 2x2x1
            let (sx, sy, sz) = (2, 2, 1);
            make_floor(&mut blocks, sx, sz);

            blocks.push(make_block(0, 1, 0, "minecraft:redstone_wire", None));
            blocks.push(make_block(
                1,
                1,
                0,
                "minecraft:repeater",
                Some(vec![("facing", "east"), ("delay", "1")]),
            ));

            Primitive {
                name: "BUF".into(),
                size_x: sx,
                size_y: sy,
                size_z: sz,
                blocks,
                input_ports: vec![(-1, 1, 0)],
                output_port: (2, 1, 0),
            }
        }

        "NOT" => {
            // Description: Block with torch on side
            // Size: 2x2x1
            let (sx, sy, sz) = (2, 2, 1);
            make_floor(&mut blocks, sx, sz);

            // Input repeater -> block
            blocks.push(make_block(
                0,
                1,
                0,
                "minecraft:repeater",
                Some(vec![("facing", "east")]),
            ));
            blocks.push(make_block(1, 1, 0, "minecraft:stone", None));
            // Output torch on the East face of the block
            blocks.push(make_block(
                2,
                1,
                0,
                "minecraft:redstone_torch",
                Some(vec![("facing", "east"), ("lit", "true")]),
            ));

            Primitive {
                name: "NOT".into(),
                size_x: sx,
                size_y: sy,
                size_z: sz,
                blocks,
                input_ports: vec![(-1, 1, 0)],
                output_port: (2, 1, 0),
            }
        }

        "OR" => {
            // Description: Two repeaters merging into one dust line
            // Size: 2x2x3
            let (sx, sy, sz) = (2, 2, 3);
            make_floor(&mut blocks, sx, sz);

            // Input A (z=0)
            blocks.push(make_block(
                0,
                1,
                0,
                "minecraft:repeater",
                Some(vec![("facing", "east")]),
            ));
            // Input B (z=2)
            blocks.push(make_block(
                0,
                1,
                2,
                "minecraft:repeater",
                Some(vec![("facing", "east")]),
            ));

            // Merging Wire
            blocks.push(make_block(1, 1, 0, "minecraft:redstone_wire", None));
            blocks.push(make_block(1, 1, 1, "minecraft:redstone_wire", None)); // Output center
            blocks.push(make_block(1, 1, 2, "minecraft:redstone_wire", None));

            Primitive {
                name: "OR".into(),
                size_x: sx,
                size_y: sy,
                size_z: sz,
                blocks,
                input_ports: vec![(-1, 1, 0), (-1, 1, 2)],
                output_port: (2, 1, 1),
            }
        }

        "AND" => {
            // Description: Invert inputs -> OR -> Invert Output (De Morgan's Laws)
            // Size: 4x3x3
            let (sx, sy, sz) = (4, 3, 3);
            make_floor(&mut blocks, sx, sz);

            // -- Input Inverters --
            // Input A (z=0)
            blocks.push(make_block(0, 1, 0, "minecraft:stone", None));
            blocks.push(make_block(
                0,
                2,
                0,
                "minecraft:redstone_torch",
                Some(vec![("lit", "true")]),
            )); // Torch on top

            // Input B (z=2)
            blocks.push(make_block(0, 1, 2, "minecraft:stone", None));
            blocks.push(make_block(
                0,
                2,
                2,
                "minecraft:redstone_torch",
                Some(vec![("lit", "true")]),
            )); // Torch on top

            // -- Wire Bridge (The "OR") --
            blocks.push(make_block(1, 1, 0, "minecraft:stone", None));
            blocks.push(make_block(1, 1, 1, "minecraft:stone", None));
            blocks.push(make_block(1, 1, 2, "minecraft:stone", None));

            blocks.push(make_block(1, 2, 0, "minecraft:redstone_wire", None));
            blocks.push(make_block(1, 2, 1, "minecraft:redstone_wire", None)); // Center point
            blocks.push(make_block(1, 2, 2, "minecraft:redstone_wire", None));

            // -- Output Inverter --
            blocks.push(make_block(2, 1, 1, "minecraft:redstone_wire", None));
            blocks.push(make_block(3, 1, 1, "minecraft:stone", None));
            // Output torch on side
            blocks.push(make_block(
                4,
                1,
                1,
                "minecraft:redstone_torch",
                Some(vec![("facing", "east"), ("lit", "true")]),
            ));

            Primitive {
                name: "AND".into(),
                size_x: sx,
                size_y: sy,
                size_z: sz,
                blocks,
                input_ports: vec![(-1, 1, 0), (-1, 1, 2)],
                output_port: (4, 1, 1),
            }
        }

        _ => Primitive {
            name: "UNKNOWN".into(),
            size_x: 1,
            size_y: 1,
            size_z: 1,
            blocks: vec![],
            input_ports: vec![],
            output_port: (0, 0, 0),
        },
    }
}
