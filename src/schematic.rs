use crate::compiler::Circuit;
use crate::layout::Layout;
use crate::primitives::primitive_for;
use anyhow::Result;
use nbt::{Map, Value};
use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn write_schem(_circuit: &Circuit, _layout: &Layout, path: &Path) -> Result<()> {
    let mut root_map = Map::new();
    root_map.insert("SubVersion".to_string(), Value::Int(1));
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let mut metadata = Map::new();
    metadata.insert("Name".to_string(), Value::String("Unnamed".to_string()));
    metadata.insert(
        "Author".to_string(),
        Value::String("schemlogica".to_string()),
    );
    metadata.insert("TimeCreated".to_string(), Value::Long(now));
    metadata.insert("TimeModified".to_string(), Value::Long(now));

    let mut region = Map::new();
    region.insert("Name".to_string(), Value::String("Unnamed".to_string()));

    let mut placed: Vec<(i32, i32, i32, String, Option<Vec<(String, String)>>)> = Vec::new();
    let mut pos_map: HashMap<String, (i32, i32, i32)> = HashMap::new();

    // Map layout positions
    for (id, lx, ly, lz) in &_layout.positions {
        pos_map.insert(id.clone(), (*lx, *ly, *lz));
    }

    // Place primitives
    // Helper functions that operate on the placed vector without capturing it
    fn place_wire_fn(
        placed: &mut Vec<(i32, i32, i32, String, Option<Vec<(String, String)>>)>,
        x: i32,
        y: i32,
        z: i32,
        dist: &mut i32,
        facing: &str,
    ) {
        placed.push((x, y - 1, z, "minecraft:glass".to_string(), None)); // Support
        *dist += 1;
        if *dist >= 15 {
            *dist = 0;
            placed.push((
                x,
                y,
                z,
                "minecraft:repeater".to_string(),
                Some(vec![("facing".to_string(), facing.to_string())]),
            ));
        } else {
            placed.push((x, y, z, "minecraft:redstone_wire".to_string(), None));
        }
    }

    fn build_stairs_fn(
        placed: &mut Vec<(i32, i32, i32, String, Option<Vec<(String, String)>>)>,
        x: i32,
        y_start: i32,
        y_end: i32,
        z_start: i32,
    ) -> i32 {
        let mut cy = y_start;
        let mut cz = z_start;
        let dy = if y_end > y_start { 1 } else { -1 };

        while cy != y_end {
            // To move 1 Y, we must move 1 horizontally (Z).
            // Step 1: Wire at current
            placed.push((x, cy - 1, cz, "minecraft:glass".to_string(), None));
            placed.push((x, cy, cz, "minecraft:redstone_wire".to_string(), None));

            // Step 2: Move Z and Y
            cy += dy;
            cz += 1; // Always move Z+ to avoid self-collision
        }
        // Final placement at target height
        placed.push((x, cy - 1, cz, "minecraft:glass".to_string(), None));
        placed.push((x, cy, cz, "minecraft:redstone_wire".to_string(), None));
        cz
    }

    for g in &_circuit.gates {
        if let Some(&(gx, gy, gz)) = pos_map.get(&g.id) {
            let prim = primitive_for(&g.kind);
            for b in prim.blocks.iter() {
                let ax = gx + b.x;
                let ay = gy + b.y;
                let az = gz + b.z;
                placed.push((ax, ay, az, b.name.clone(), b.properties.clone()));
            }
        }
    }

    // Routing
    let mut signal_output_pos: HashMap<String, (i32, i32, i32)> = HashMap::new();
    for g in &_circuit.gates {
        if let Some(&(gx, gy, gz)) = pos_map.get(&g.id) {
            let prim = primitive_for(&g.kind);
            let (ox, oy, oz) = prim.output_port;
            signal_output_pos.insert(g.output.clone(), (gx + ox, gy + oy, gz + oz));
        }
    }

    // Wiring Plan:
    // To avoid collisions, we assign each wire a unique "lane" (Y level).
    // Start wires high up (Y=5) to avoid hitting gates (Y=0..3).
    let mut next_wire_y = 5;

    for g in &_circuit.gates {
        if let Some(&(gx, gy, gz)) = pos_map.get(&g.id) {
            let prim = primitive_for(&g.kind);
            for (i_idx, in_port) in prim.input_ports.iter().enumerate() {
                if let Some(src_sig) = g.inputs.get(i_idx) {
                    if let Some(&(sx, sy, sz)) = signal_output_pos.get(src_sig) {
                        let (ix, iy, iz) = (gx + in_port.0, gy + in_port.1, gz + in_port.2);

                        let lane_y = next_wire_y;
                        next_wire_y += 1;

                        // (helpers are above as functions to avoid borrowing placed twice)

                        // --- Execution ---
                        let mut signal_dist = 0;

                        // 1. Riser: Output (sy) -> Lane (lane_y)
                        // We staircase "out" in Z to avoid hitting the gate itself.
                        let z_lane_start = build_stairs_fn(&mut placed, sx, sy, lane_y, sz);

                        // 2. Horizontal: Move X (sx -> ix) at lane_y
                        let mut cx = sx;
                        let dx = if ix >= sx { 1 } else { -1 };
                        let facing = if dx > 0 { "east" } else { "west" };

                        // First match X
                        while cx != ix {
                            cx += dx;
                            place_wire_fn(
                                &mut placed,
                                cx,
                                lane_y,
                                z_lane_start,
                                &mut signal_dist,
                                facing,
                            );
                        }

                        // 3. Horizontal: Move Z (z_lane_start -> iz) at lane_y?
                        // Wait, we need to arrive at `iz` eventually.
                        // But we also need to staircase down from `lane_y` to `iy`.
                        // Let's travel Z until we are close enough?
                        // Actually, just travel to `iz` (adjusted for the staircase length needed).
                        // Staircase length = abs(lane_y - iy).
                        let stairs_len = (lane_y - iy).abs();
                        // Target Z for the top of the down-stairs
                        let z_pre_drop = iz - stairs_len;

                        // Move Z from current (z_lane_start) to z_pre_drop
                        let mut cz = z_lane_start;
                        let dz = if z_pre_drop >= z_lane_start { 1 } else { -1 };
                        let z_face = if dz > 0 { "south" } else { "north" };

                        while cz != z_pre_drop {
                            cz += dz;
                            place_wire_fn(&mut placed, cx, lane_y, cz, &mut signal_dist, z_face);
                        }

                        // 4. Drop: Lane (lane_y) -> Input (iy)
                        // Using staircase logic, which naturally moves Z+ as it goes down.
                        // We aimed for `iz - stairs_len`, so adding `stairs_len` (from Z moves) lands us at `iz`.
                        let final_z = build_stairs_fn(&mut placed, cx, lane_y, iy, cz);

                        // Connect final tip to input port?
                        // The staircase places wire at (cx, iy, final_z).
                        // The input port is at (ix, iy, iz).
                        // Due to our calc, final_z should approx equals iz.
                        // Just run a tiny wire to connect if off by 1.
                        if final_z != iz {
                            let dz2 = if iz >= final_z { 1 } else { -1 };
                            let mut cz2 = final_z;
                            while cz2 != iz {
                                cz2 += dz2;
                                place_wire_fn(&mut placed, cx, iy, cz2, &mut signal_dist, "south");
                            }
                        }
                    }
                }
            }
        }
    }

    // Bounds calculation
    let (min_x, min_y, min_z, max_x, max_y, max_z) = if placed.is_empty() {
        (0, 0, 0, 0, 0, 0)
    } else {
        let (mut mx, mut my, mut mz, mut Mx, mut My, mut Mz) =
            (i32::MAX, i32::MAX, i32::MAX, i32::MIN, i32::MIN, i32::MIN);
        for (x, y, z, _, _) in &placed {
            if *x < mx {
                mx = *x
            }
            if *y < my {
                my = *y
            }
            if *z < mz {
                mz = *z
            }
            if *x > Mx {
                Mx = *x
            }
            if *y > My {
                My = *y
            }
            if *z > Mz {
                Mz = *z
            }
        }
        (mx, my, mz, Mx, My, Mz)
    };

    let width = max_x - min_x + 1;
    let height = max_y - min_y + 1;
    let length = max_z - min_z + 1;

    // Palette Building
    fn canonical_key(name: &str, props: &Option<Vec<(String, String)>>) -> String {
        let mut key = name.to_string();
        if let Some(p) = props {
            let mut ps = p.clone();
            ps.sort_by(|a, b| a.0.cmp(&b.0));
            for (k, v) in ps {
                key.push_str(&format!("|{}={}", k, v));
            }
        }
        key
    }

    let mut palette_keys = vec![("minecraft:air".to_string(), None)];
    let mut palette_index = HashMap::new();
    palette_index.insert(canonical_key("minecraft:air", &None), 0usize);

    for (_, _, _, name, props) in &placed {
        let key = canonical_key(name, props);
        if let std::collections::hash_map::Entry::Vacant(e) = palette_index.entry(key) {
            let idx = palette_keys.len();
            e.insert(idx);
            palette_keys.push((name.clone(), props.clone()));
        }
    }

    // BlockStates
    let mut indices: Vec<u32> = Vec::with_capacity((width * height * length) as usize);
    for y in 0..height {
        for z in 0..length {
            for x in 0..width {
                let (wx, wy, wz) = (min_x + x, min_y + y, min_z + z);
                let mut found = 0;
                for (bx, by, bz, name, props) in &placed {
                    if *bx == wx && *by == wy && *bz == wz {
                        let key = canonical_key(name, props);
                        found = *palette_index.get(&key).unwrap_or(&0) as u32;
                        break;
                    }
                }
                indices.push(found);
            }
        }
    }

    // Bit Packing
    let bits = ((palette_keys.len() as f64).log2().ceil() as usize).max(2);
    let mut longs = Vec::new();
    let _mask = (1u128 << bits) - 1;
    let mut acc = 0u128;
    let mut acc_bits = 0;

    for idx in indices {
        acc |= (idx as u128) << acc_bits;
        acc_bits += bits;
        while acc_bits >= 64 {
            longs.push((acc & 0xFFFF_FFFF_FFFF_FFFF) as i64);
            acc >>= 64;
            acc_bits -= 64;
        }
    }
    if acc_bits > 0 {
        longs.push(acc as i64);
    }

    // Region Construction
    let mut pos_tag = Map::new();
    pos_tag.insert("x".into(), Value::Int(min_x));
    pos_tag.insert("y".into(), Value::Int(min_y));
    pos_tag.insert("z".into(), Value::Int(min_z));
    region.insert("Position".into(), Value::Compound(pos_tag));

    let mut size_tag = Map::new();
    size_tag.insert("x".into(), Value::Int(width));
    size_tag.insert("y".into(), Value::Int(height));
    size_tag.insert("z".into(), Value::Int(length));
    region.insert("Size".into(), Value::Compound(size_tag));

    let mut pal_list = Vec::new();
    for (name, props) in palette_keys {
        let mut entry = Map::new();
        entry.insert("Name".into(), Value::String(name));
        if let Some(p) = props {
            let mut pm = Map::new();
            for (k, v) in p {
                pm.insert(k, Value::String(v));
            }
            entry.insert("Properties".into(), Value::Compound(pm));
        }
        pal_list.push(Value::Compound(entry));
    }
    region.insert("BlockStatePalette".into(), Value::List(pal_list));
    region.insert("BlockStates".into(), Value::LongArray(longs));
    region.insert("PendingBlockTicks".into(), Value::List(vec![]));
    region.insert("TileEntities".into(), Value::List(vec![]));
    region.insert("Entities".into(), Value::List(vec![]));

    let mut regions = Map::new();
    regions.insert("Unnamed".into(), Value::Compound(region));
    root_map.insert("Regions".into(), Value::Compound(regions));

    // Metadata
    metadata.insert("TotalBlocks".into(), Value::Int(placed.len() as i32));
    metadata.insert("TotalVolume".into(), Value::Int(width * height * length));
    let mut enc = Map::new();
    enc.insert("x".into(), Value::Int(width));
    enc.insert("y".into(), Value::Int(height));
    enc.insert("z".into(), Value::Int(length));
    metadata.insert("EnclosingSize".into(), Value::Compound(enc));
    root_map.insert("Metadata".into(), Value::Compound(metadata));
    root_map.insert("MinecraftDataVersion".into(), Value::Int(2586)); // 1.16.5
    root_map.insert("Version".into(), Value::Int(5));

    let mut blob = nbt::Blob::new();
    for (k, v) in root_map {
        blob.insert(k, v)?;
    }

    let file = File::create(path)?;
    blob.to_gzip_writer(&mut std::io::BufWriter::new(file))?;
    Ok(())
}
