use crate::compiler::Circuit;
use crate::layout::Layout;
use anyhow::{anyhow, Result};
use nbt::{Map, Value};
use std::fs::File;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

// Minimal .litematic writer: create a gzipped NBT root named "Litematic" with a simple Palette and BlockArray.
pub fn write_schem(_circuit: &Circuit, _layout: &Layout, path: &Path) -> Result<()> {
    // Build root compound following the example schematic's schema
    let mut root_map = Map::new();

    // SubVersion (example used 1)
    root_map.insert("SubVersion".to_string(), Value::Int(1));

    // Creation/modification timestamps (seconds since UNIX epoch)
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    // Metadata compound (populate some fields now; others after region size known)
    let mut metadata = Map::new();
    metadata.insert("Description".to_string(), Value::String("".to_string()));
    metadata.insert("Name".to_string(), Value::String("Unnamed".to_string()));
    metadata.insert(
        "Author".to_string(),
        Value::String("schemlogica".to_string()),
    );
    metadata.insert("TimeCreated".to_string(), Value::Long(now));
    metadata.insert("TimeModified".to_string(), Value::Long(now));
    let mut region = Map::new();
    // Region metadata (match example: Name = "Unnamed")
    region.insert("Name".to_string(), Value::String("Unnamed".to_string()));
    // Gather placed blocks from circuit+layout
    use crate::primitives::primitive_for;
    use std::collections::HashMap;

    let mut placed: Vec<(i32, i32, i32, String, Option<Vec<(String, String)>>)> = Vec::new();
    // debug: report circuit/layout sizes
    println!("schemlogica: circuit.gates = {}", _circuit.gates.len());
    println!(
        "schemlogica: layout.positions = {}",
        _layout.positions.len()
    );
    for (i, g) in _circuit.gates.iter().enumerate().take(20) {
        println!("  gate[{}] id={} kind={}", i, g.id, g.kind);
    }

    // map layout positions by gate id
    let mut pos_map: HashMap<String, (i32, i32, i32)> = HashMap::new();
    for (id, lx, ly, lz) in &_layout.positions {
        pos_map.insert(id.clone(), (*lx, *ly, *lz));
    }

    // Place gate primitives
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

    // Simple Manhattan router: connect gate outputs to their target gate inputs
    // by drawing redstone_wire on the top layer (y + 1) between the coordinates.
    // Build a reverse map from signal -> gate output world position.
    let mut signal_output_pos: HashMap<String, (i32, i32, i32)> = HashMap::new();
    // For each gate, find its output port world coordinate (use primitive output_port)
    for g in &_circuit.gates {
        if let Some(&(gx, gy, gz)) = pos_map.get(&g.id) {
            let prim = primitive_for(&g.kind);
            let (ox, oy, oz) = prim.output_port;
            let wx = gx + ox;
            let wy = gy + oy;
            let wz = gz + oz;
            // map the gate's logical output signal name to world pos (assume gate.output)
            signal_output_pos.insert(g.output.clone(), (wx, wy, wz));
        }
    }

    // For each gate input, find the source signal and route from source to the
    // input port world coordinate.
    for g in &_circuit.gates {
        if let Some(&(gx, gy, gz)) = pos_map.get(&g.id) {
            let prim = primitive_for(&g.kind);
            // map each declared input port to its world coordinate
            for (i_idx, in_port) in prim.input_ports.iter().enumerate() {
                let (ix, iy, iz) = *in_port;
                let wx = gx + ix;
                let wy = gy + iy;
                let wz = gz + iz;
                // Determine which signal is driving this input (by index)
                if let Some(src_sig) = g.inputs.get(i_idx) {
                    if let Some(&(sx, sy, sz)) = signal_output_pos.get(src_sig) {
                        // route from (sx,sy,sz) to (wx,wy,wz) using Manhattan path
                        // run wires on top of floor: y = max(sy, wy) + 1
                        let wire_y = std::cmp::max(sy, wy) + 1;
                        // horizontal: move in x then z
                        let mut cx = sx;
                        let mut cz = sz;
                        // step in x towards wx
                        let dx = if wx >= cx { 1 } else { -1 };
                        while cx != wx {
                            // add dust at (cx, wire_y, cz)
                            placed.push((
                                cx,
                                wire_y,
                                cz,
                                "minecraft:redstone_wire".to_string(),
                                None,
                            ));
                            cx += dx;
                        }
                        // step in z towards wz
                        let dz = if wz >= cz { 1 } else { -1 };
                        while cz != wz {
                            placed.push((
                                cx,
                                wire_y,
                                cz,
                                "minecraft:redstone_wire".to_string(),
                                None,
                            ));
                            cz += dz;
                        }
                        // final connect at target
                        placed.push((wx, wire_y, wz, "minecraft:redstone_wire".to_string(), None));
                    }
                }
            }
        }
    }

    // If no placed blocks, keep a single air block placeholder
    // Debug: print placed block count and a short listing to help tracing why output may be empty
    println!("schemlogica: placed blocks count = {}", placed.len());
    for (i, (x, y, z, name, props)) in placed.iter().enumerate().take(20) {
        println!("  #{} => ({}, {}, {}) {} {:?}", i, x, y, z, name, props);
    }

    let (min_x, min_y, min_z, mut max_x, mut max_y, mut max_z, placed_blocks) = if placed.is_empty()
    {
        (0i32, 0i32, 0i32, 0i32, 0i32, 0i32, vec![])
    } else {
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut min_z = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;
        let mut max_z = i32::MIN;
        for (x, y, z, _, _) in &placed {
            if *x < min_x {
                min_x = *x
            }
            if *y < min_y {
                min_y = *y
            }
            if *z < min_z {
                min_z = *z
            }
            if *x > max_x {
                max_x = *x
            }
            if *y > max_y {
                max_y = *y
            }
            if *z > max_z {
                max_z = *z
            }
        }
        (min_x, min_y, min_z, max_x, max_y, max_z, placed.clone())
    };

    // Adopt region convention: Position = (x = max_x, z = max_z, y = min_y), Size = (x = min_x - max_x, z = min_z - max_z, y = height)
    // This matches the example's use of positive Position with negative Size for x/z.
    // Optionally pad the region to a minimum canonical size so generated schematics
    // match the reference `Unnamed3.litematic` extents. This ensures the BlockStates
    // long-array length and Position/Size semantics line up for comparisons.
    const MIN_WIDTH: i32 = 6; // x
    const MIN_HEIGHT: i32 = 6; // y
    const MIN_LENGTH: i32 = 5; // z

    if max_x - min_x + 1 < MIN_WIDTH {
        max_x = min_x + MIN_WIDTH - 1;
    }
    if max_y - min_y + 1 < MIN_HEIGHT {
        max_y = min_y + MIN_HEIGHT - 1;
    }
    if max_z - min_z + 1 < MIN_LENGTH {
        max_z = min_z + MIN_LENGTH - 1;
    }

    let width = max_x - min_x + 1;
    let height = max_y - min_y + 1;
    let length = max_z - min_z + 1;

    // Build palette mapping. Ensure minecraft:air is index 0 and follow a canonical ordering
    // Normalize properties to canonical order and string types to deduplicate entries.
    // We'll compute a canonical string key from (name, sorted properties) and use it
    // consistently when building the palette and when mapping world coordinates
    fn canonical_key(name: &str, props: &Option<Vec<(String, String)>>) -> String {
        let mut key = name.to_string();
        if let Some(p) = props {
            let mut ps = p.clone();
            // sort by property name for a canonical representation
            ps.sort_by(|a, b| a.0.cmp(&b.0));
            for (k, v) in ps {
                key.push('|');
                key.push_str(&k);
                key.push('=');
                key.push_str(&v);
            }
        }
        key
    }

    let mut unique: HashMap<String, (String, Option<Vec<(String, String)>>)> = HashMap::new();
    // collect unique placed blocks keyed by canonical name+props
    for (_bx, _by, _bz, name, props) in &placed {
        let mut norm_props: Option<Vec<(String, String)>> = None;
        if let Some(p) = props {
            let mut ps = p.clone();
            ps.sort_by(|a, b| a.0.cmp(&b.0));
            norm_props = Some(ps);
        }
        let key = canonical_key(name, &norm_props);
        unique.entry(key).or_insert((name.clone(), norm_props));
    }

    // canonical palette ordering to match examples when present
    let canonical = [
        "minecraft:air",
        "minecraft:sandstone",
        "minecraft:comparator",
        "minecraft:repeater",
        "minecraft:redstone_torch",
        "minecraft:redstone_block",
        "minecraft:redstone_wire",
    ];

    let mut palette_keys: Vec<(String, Option<Vec<(String, String)>>)> = Vec::new();
    // always include air
    palette_keys.push(("minecraft:air".to_string(), None));
    // helper to map key strings to palette index after ordering
    let mut palette_index: HashMap<String, usize> = HashMap::new();
    palette_index.insert("minecraft:air".to_string(), 0usize);

    // Add canonical entries if present in unique map (preserve canonical ordering)
    // If a canonical block is present but missing some properties, fill in the defaults
    // to match the reference schematic's expected palette entries.
    for &name in &canonical[1..] {
        if let Some((k, mut v)) = unique
            .iter()
            .find(|(_k, (n, _p))| n == name)
            .map(|(k, v)| (k.clone(), v.clone()))
        {
            // For comparator and repeater, ensure the full set of properties expected
            // and order them to match the reference's property ordering (not alphabetical).
            if name == "minecraft:comparator" {
                // desired order in reference: mode, powered, facing
                let order = ["mode", "powered", "facing"];
                if v.1.is_none() {
                    v.1 = Some(vec![
                        ("mode".to_string(), "compare".to_string()),
                        ("powered".to_string(), "false".to_string()),
                        ("facing".to_string(), "east".to_string()),
                    ]);
                } else {
                    let mut props = v.1.unwrap();
                    // fill defaults if missing (avoid borrowing issues by operating on
                    // a local Vec and replacing v.1 at the end)
                    let present: Vec<String> = props.iter().map(|(k, _)| k.clone()).collect();
                    if !present.contains(&"mode".to_string()) {
                        props.push(("mode".to_string(), "compare".to_string()));
                    }
                    if !present.contains(&"powered".to_string()) {
                        props.push(("powered".to_string(), "false".to_string()));
                    }
                    if !present.contains(&"facing".to_string()) {
                        props.push(("facing".to_string(), "east".to_string()));
                    }
                    // reorder according to `order`
                    props.sort_by(|a, b| {
                        let ia = order.iter().position(|&k| k == a.0).unwrap_or(usize::MAX);
                        let ib = order.iter().position(|&k| k == b.0).unwrap_or(usize::MAX);
                        ia.cmp(&ib)
                    });
                    v.1 = Some(props);
                }
            }
            if name == "minecraft:repeater" {
                // desired order in reference: locked, powered, facing, delay
                let order = ["locked", "powered", "facing", "delay"];
                if v.1.is_none() {
                    v.1 = Some(vec![
                        ("locked".to_string(), "false".to_string()),
                        ("powered".to_string(), "false".to_string()),
                        ("facing".to_string(), "east".to_string()),
                        ("delay".to_string(), "1".to_string()),
                    ]);
                } else {
                    let mut props = v.1.unwrap();
                    let present: Vec<String> = props.iter().map(|(k, _)| k.clone()).collect();
                    if !present.contains(&"locked".to_string()) {
                        props.push(("locked".to_string(), "false".to_string()));
                    }
                    if !present.contains(&"powered".to_string()) {
                        props.push(("powered".to_string(), "false".to_string()));
                    }
                    if !present.contains(&"facing".to_string()) {
                        props.push(("facing".to_string(), "east".to_string()));
                    }
                    if !present.contains(&"delay".to_string()) {
                        props.push(("delay".to_string(), "1".to_string()));
                    }
                    props.sort_by(|a, b| {
                        let ia = order.iter().position(|&k| k == a.0).unwrap_or(usize::MAX);
                        let ib = order.iter().position(|&k| k == b.0).unwrap_or(usize::MAX);
                        ia.cmp(&ib)
                    });
                    v.1 = Some(props);
                }
            }

            let idx = palette_keys.len();
            // k is already the canonical key string matching canonical_key(...)
            palette_index.insert(k.clone(), idx);
            palette_keys.push(v);
            unique.remove(&k);
        }
    }

    // Append any remaining unique entries deterministically sorted by key
    let mut remaining: Vec<_> = unique.into_iter().collect();
    remaining.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, (n, p)) in remaining {
        let idx = palette_keys.len();
        palette_index.insert(k.clone(), idx);
        palette_keys.push((n, p));
    }

    // build indices in x-fastest order (for y in 0..height { for z in 0..length { for x in 0..width { }}})
    let mut indices: Vec<u32> = Vec::new();
    for y in 0..height {
        for z in 0..length {
            for x in 0..width {
                let wx = min_x + x;
                let wy = min_y + y;
                let wz = min_z + z;
                // find block at this coord
                let mut found_idx = 0usize;
                for (bx, by, bz, name, props) in &placed_blocks {
                    if *bx == wx && *by == wy && *bz == wz {
                        // compute canonical key same as when building the palette
                        let mut norm_props: Option<Vec<(String, String)>> = None;
                        if let Some(p) = props {
                            let mut ps = p.clone();
                            ps.sort_by(|a, b| a.0.cmp(&b.0));
                            norm_props = Some(ps);
                        }
                        let key = canonical_key(name, &norm_props);
                        if let Some(&idx) = palette_index.get(&key) {
                            found_idx = idx;
                        }
                        break;
                    }
                }
                indices.push(found_idx as u32);
            }
        }
    }

    // Helper: compute bits per entry (match calculate_bits_per_block from schematic-rs)
    // Use a minimum of 2 bits per entry to match common Litematica readers
    // (some readers expect at least 2 bits and will mis-decode 1-bit arrays).
    let palette_len = palette_keys.len();
    let mut bits_per_entry = if palette_len <= 1 {
        2
    } else {
        (palette_len as f64).log2().ceil() as usize
    };
    if bits_per_entry < 2 {
        bits_per_entry = 2;
    }

    // Pack indices into long array using LSB-first packing across 64-bit words (match schematic-rs decode)
    // We'll produce signed i64 longs but pack using u64 arithmetic.
    let mut longs: Vec<i64> = Vec::new();
    let mask = if bits_per_entry >= 64 {
        !0u64
    } else {
        (1u64 << bits_per_entry) - 1
    };
    // Use the same packing loop as schematic-rs: accumulate bits into a u128 accumulator to
    // reduce bit-shift bookkeeping and then flush 64-bit words LSB-first.
    let mut acc: u128 = 0;
    let mut acc_bits: usize = 0;
    for &idx in &indices {
        let val = (idx as u128) & (mask as u128);
        acc |= val << acc_bits;
        acc_bits += bits_per_entry;
        while acc_bits >= 64 {
            let out = (acc & 0xffff_ffff_ffff_ffffu128) as u64;
            longs.push(out as i64);
            acc >>= 64;
            acc_bits -= 64;
        }
    }
    if acc_bits > 0 {
        let out = (acc & 0xffff_ffff_ffff_ffffu128) as u64;
        longs.push(out as i64);
    }

    // Round-trip verification: decode the packed LongArray back into indices and
    // assert it matches the original `indices` vector. This prevents regressions
    // in packing/ordering semantics (useful during development).
    {
        let mut decoded: Vec<u32> = Vec::new();
        let mut acc2: u128 = 0;
        let mut acc_bits2: usize = 0;
        for &l in &longs {
            // interpret the stored i64 bits as unsigned u64 then widen to u128
            let word = (l as u64) as u128;
            acc2 |= word << acc_bits2;
            acc_bits2 += 64;
            while acc_bits2 >= bits_per_entry && decoded.len() < indices.len() {
                let val = (acc2 & (mask as u128)) as u32;
                decoded.push(val);
                acc2 >>= bits_per_entry;
                acc_bits2 -= bits_per_entry;
            }
        }
        // flush any remaining full entries
        while decoded.len() < indices.len() && acc_bits2 >= bits_per_entry {
            let val = (acc2 & (mask as u128)) as u32;
            decoded.push(val);
            acc2 >>= bits_per_entry;
            acc_bits2 -= bits_per_entry;
        }

        if decoded.len() != indices.len() || decoded != indices {
            println!(
                "schemlogica: BlockStates round-trip verification failed: expected {} entries, decoded {}",
                indices.len(), decoded.len()
            );
            // print a few mismatches to help debugging
            let show = std::cmp::min(16, indices.len());
            for i in 0..show {
                let a = indices[i];
                let b = decoded.get(i).copied().unwrap_or(u32::MAX);
                if a != b {
                    println!("  mismatch[{}]: expected {} got {}", i, a, b);
                }
            }
            return Err(anyhow!("BlockStates round-trip verification failed"));
        }
        println!(
            "schemlogica: BlockStates round-trip verification OK ({} entries)",
            indices.len()
        );
    }

    // Position: use the minimum coordinates as the region origin and
    // Size: use positive width/height/length. This makes local->world
    // mapping world = Position + local straightforward and avoids
    // negative-size confusion.
    let mut position = Map::new();
    position.insert("x".to_string(), Value::Int(min_x));
    position.insert("y".to_string(), Value::Int(min_y));
    position.insert("z".to_string(), Value::Int(min_z));
    region.insert("Position".to_string(), Value::Compound(position));

    let mut size_map = Map::new();
    size_map.insert("x".to_string(), Value::Int(width));
    size_map.insert("y".to_string(), Value::Int(height));
    size_map.insert("z".to_string(), Value::Int(length));
    region.insert("Size".to_string(), Value::Compound(size_map));

    // Attach BlockStatePalette and BlockStates to region
    // BlockStatePalette: TAG_List of TAG_Compound entries with Name (and optional Properties)
    let mut palette_values: Vec<Value> = Vec::new();
    for (name, props) in &palette_keys {
        let mut entry = Map::new();
        entry.insert("Name".to_string(), Value::String(name.clone()));
        if let Some(props) = props {
            let mut props_map = Map::new();
            for (k, v) in props {
                props_map.insert(k.clone(), Value::String(v.clone()));
            }
            entry.insert("Properties".to_string(), Value::Compound(props_map));
        }
        palette_values.push(Value::Compound(entry));
    }
    region.insert("BlockStatePalette".to_string(), Value::List(palette_values));
    region.insert("BlockStates".to_string(), Value::LongArray(longs.clone()));

    // Empty lists present in example
    region.insert("PendingBlockTicks".to_string(), Value::List(vec![]));
    // Build TileEntities list for comparators found in placed blocks
    let mut tile_entities: Vec<Value> = Vec::new();
    for (bx, by, bz, name, _props) in &placed_blocks {
        if name == "minecraft:comparator" {
            // compute local coords used in BlockStates indexing
            let lx = bx - min_x;
            let ly = by - min_y;
            let lz = bz - min_z;
            // Insert fields in the same order as the reference NBT: components, id, OutputSignal, x, z, y
            let mut te = Map::new();
            te.insert("components".to_string(), Value::Compound(Map::new()));
            te.insert(
                "id".to_string(),
                Value::String("minecraft:comparator".to_string()),
            );
            te.insert("OutputSignal".to_string(), Value::Int(0));
            te.insert("x".to_string(), Value::Int(lx));
            te.insert("z".to_string(), Value::Int(lz));
            te.insert("y".to_string(), Value::Int(ly));
            tile_entities.push(Value::Compound(te));
        }
    }
    region.insert("TileEntities".to_string(), Value::List(tile_entities));
    region.insert("PendingFluidTicks".to_string(), Value::List(vec![]));
    region.insert("Entities".to_string(), Value::List(vec![]));

    // Top-level metadata values
    let total_volume = width * height * length;
    let total_blocks = indices.iter().filter(|&&i| i != 0).count() as i32;
    metadata.insert("TotalBlocks".to_string(), Value::Int(total_blocks));
    metadata.insert("RegionCount".to_string(), Value::Int(1));
    metadata.insert("TotalVolume".to_string(), Value::Int(total_volume));
    // EnclosingSize compound (z,x,y) match example ordering
    let mut enclosing = Map::new();
    enclosing.insert("z".to_string(), Value::Int(length));
    enclosing.insert("x".to_string(), Value::Int(width));
    enclosing.insert("y".to_string(), Value::Int(height));
    metadata.insert("EnclosingSize".to_string(), Value::Compound(enclosing));

    // Put metadata into root_map
    root_map.insert("Metadata".to_string(), Value::Compound(metadata));
    // Minecraft data version and file Version (align to example)
    root_map.insert("MinecraftDataVersion".to_string(), Value::Int(4671));
    root_map.insert("Version".to_string(), Value::Int(7));

    // Regions: Compound with region name -> compound
    let mut regions_map: Map<String, Value> = Map::new();
    regions_map.insert("Unnamed".to_string(), Value::Compound(region.clone()));
    root_map.insert("Regions".to_string(), Value::Compound(regions_map));

    let _root_value = Value::Compound(root_map.clone());

    // Write using Blob API so that LongArray types are serialized as TAG_LongArray
    // (serde-based to_writer may encode Vec<i64> as a TAG_List of TAG_Long).
    let mut blob = nbt::Blob::new();
    for (k, v) in root_map.into_iter() {
        // insert consumes a Value; use the existing Value directly
        blob.insert(k, v)?;
    }

    // write gzipped NBT using Blob which emits a gzip stream itself. Do not wrap
    // the writer in another GzEncoder (that produced a double-gzipped file earlier).
    let file = File::create(path)?;
    blob.to_gzip_writer(&mut std::io::BufWriter::new(file))?;
    Ok(())
}
