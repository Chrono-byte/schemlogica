use crate::compiler::Circuit;
use crate::layout::Layout;
use crate::primitives::primitive_for;
use anyhow::Result;
use nbt::{Map, Value};
use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

// Routing constants
const REDSTONE_SIGNAL_LIMIT: i32 = 15;
const REPEATER_THRESHOLD: i32 = 14;
const WIRE_LANE_START_Y: i32 = 4;
const WIRE_Y_SPACING: i32 = 2; // Vertical spacing between wire lanes

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
        if *dist >= REPEATER_THRESHOLD {
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
    let mut signal_source_gate: HashMap<String, String> = HashMap::new();
    for g in &_circuit.gates {
        if let Some(&(gx, gy, gz)) = pos_map.get(&g.id) {
            let prim = primitive_for(&g.kind);
            let (ox, oy, oz) = prim.output_port;
            signal_output_pos.insert(g.output.clone(), (gx + ox, gy + oy, gz + oz));
            signal_source_gate.insert(g.output.clone(), g.id.clone());
        }
    }

    // --- Flat Routing Strategy ---
    // Use A* pathfinding to route wires on the ground (Y=1) around obstacles.
    // 1. Mark all gate blocks as obstacles.
    // 2. Route wires sequentially using A*.
    // 3. Mark placed wires as new obstacles.

    // Grid management
    let mut grid_obstacles: std::collections::HashSet<(i32, i32)> =
        std::collections::HashSet::new();

    // Mark gates as obstacles
    for g in &_circuit.gates {
        if let Some(&(gx, gy, gz)) = pos_map.get(&g.id) {
            let prim = primitive_for(&g.kind);
            // Mark the footprint. previously we added a 1-block negative padding
            // around primitives which caused ports to be embedded inside obstacles.
            // Reduce padding to 0 to give ports more room (helps routing).
            let pad_x_before = 0; // was -1
            let pad_z_before = 0; // was -1
            for x in pad_x_before..=prim.size_x {
                for z in pad_z_before..=prim.size_z {
                    grid_obstacles.insert((gx + x, gz + z));
                }
            }
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
    struct Point {
        x: i32,
        z: i32,
    }

    impl Point {
        fn dist(&self, other: &Point) -> i32 {
            (self.x - other.x).abs() + (self.z - other.z).abs()
        }
    }

    // A* Pathfinding
    fn find_path(
        start: Point,
        end: Point,
        obstacles: &std::collections::HashSet<(i32, i32)>,
    ) -> Option<Vec<Point>> {
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;

        // Priority queue holds (cost+heuristic, cost, point)
        let mut open_set = BinaryHeap::new();
        open_set.push(Reverse((0, 0, start)));

        let mut came_from: HashMap<Point, Point> = HashMap::new();
        let mut g_score: HashMap<Point, i32> = HashMap::new();
        g_score.insert(start, 0);

        let mut close_set = std::collections::HashSet::new();

        // Safety Break (don't search forever)
        let max_steps = 10000;
        let mut steps = 0;

        while let Some(Reverse((_, current_g, current))) = open_set.pop() {
            steps += 1;
            // if steps > max_steps { return None; } // remove limit for reliable outputs

            if current == end {
                // Reconstruct path
                let mut path = vec![current];
                let mut curr = current;
                while let Some(&prev) = came_from.get(&curr) {
                    path.push(prev);
                    curr = prev;
                }
                path.reverse();
                return Some(path);
            }

            close_set.insert(current);

            // Neighbors (4 directions)
            let neighbors = [
                Point {
                    x: current.x + 1,
                    z: current.z,
                },
                Point {
                    x: current.x - 1,
                    z: current.z,
                },
                Point {
                    x: current.x,
                    z: current.z + 1,
                },
                Point {
                    x: current.x,
                    z: current.z - 1,
                },
            ];

            for &next in &neighbors {
                if close_set.contains(&next) {
                    continue;
                }

                // Check obstacles (except for end point, which might be "in" a gate port)
                if next != end && obstacles.contains(&(next.x, next.z)) {
                    continue;
                }

                let tentative_g = current_g + 1;

                if tentative_g < *g_score.get(&next).unwrap_or(&i32::MAX) {
                    came_from.insert(next, current);
                    g_score.insert(next, tentative_g);
                    let f_score = tentative_g + next.dist(&end);
                    open_set.push(Reverse((f_score, tentative_g, next)));
                }
            }
        }
        None
    }

    // Collect signals
    struct Connection {
        src: Point,
        dst: Point,
        src_y: i32,
        dst_y: i32,
    }
    let mut connections = Vec::new();

    for g in &_circuit.gates {
        if let Some(&(gx, gy, gz)) = pos_map.get(&g.id) {
            let prim = primitive_for(&g.kind);
            for (i_idx, in_port) in prim.input_ports.iter().enumerate() {
                if let Some(src_sig) = g.inputs.get(i_idx) {
                    if let Some(&(sx, sy, sz)) = signal_output_pos.get(src_sig) {
                        let (ix, iy, iz) = (gx + in_port.0, gy + in_port.1, gz + in_port.2);
                        connections.push(Connection {
                            src: Point { x: sx, z: sz },
                            dst: Point { x: ix, z: iz },
                            src_y: sy,
                            dst_y: iy,
                        });

                        // Diagnostic: if the Manhattan distance is large, print details
                        let manhattan = (sx - ix).abs() + (sz - iz).abs();
                        if manhattan > 20 {
                            let src_gate = signal_source_gate
                                .get(src_sig)
                                .cloned()
                                .unwrap_or("<unknown>".to_string());
                            eprintln!("Long connection (distance {}) for signal '{}' from gate '{}' @ ({},{}) to gate '{}' @ ({},{})",
                                manhattan, src_sig, src_gate, sx, sz, g.id, ix, iz);
                        }
                    }
                }
            }
        }
    }

    // Sort connections by length (heuristic) to route short ones first?
    // Or maybe route long ones first?
    // Let's just route in order.

    for conn in connections {
        // Clear obstacles at start/end to ensure connectivity
        // (Sometimes ports are inside the "block footprint" padding)
        // Actually, find_path already allows end point.

        // Allow start/end positions to be considered free even if they lie inside
        // the padded gate footprints. Clone the obstacle set and clear the endpoints
        // so A* can start or finish inside what was marked as an obstacle.
        let mut local_obs = grid_obstacles.clone();
        local_obs.remove(&(conn.src.x, conn.src.z));
        local_obs.remove(&(conn.dst.x, conn.dst.z));

        if let Some(path) = find_path(conn.src, conn.dst, &local_obs) {
            // Place path
            let mut signal_dist = 0;

            for (idx, p) in path.iter().enumerate() {
                // Determine direction for repeaters
                let facing = if idx + 1 < path.len() {
                    let next = path[idx + 1];
                    if next.x > p.x {
                        "east"
                    } else if next.x < p.x {
                        "west"
                    } else if next.z > p.z {
                        "south"
                    } else {
                        "north"
                    }
                } else {
                    "north" // default
                };

                // Add to obstacles for future wires
                grid_obstacles.insert((p.x, p.z));

                // Place wire or repeater
                // Don't place on top of start/end if they are higher up?
                // Logic:
                // If this is the START point:
                //   If src_y > 1, we need to bridge down.
                //   The path[0] is at (src_x, src_z) at Y=1.
                //   We need to ensure connection from (src_x, src_y, src_z) to (src_x, 1, src_z).

                let is_start = idx == 0;
                let is_end = idx == path.len() - 1;

                place_wire_fn(&mut placed, p.x, 1, p.z, &mut signal_dist, facing);

                // Handle vertical transitions at endpoints
                if is_start && conn.src_y > 1 {
                    // Vertical drop from src_y to 1
                    let mut cy = conn.src_y;
                    while cy > 1 {
                        placed.push((p.x, cy - 1, p.z, "minecraft:glass".to_string(), None));
                        placed.push((p.x, cy, p.z, "minecraft:redstone_wire".to_string(), None));
                        cy -= 1;
                    }
                }

                if is_end && conn.dst_y > 1 {
                    // Vertical rise from 1 to dst_y
                    let mut cy = 1;
                    while cy < conn.dst_y {
                        placed.push((p.x, cy, p.z, "minecraft:glass".to_string(), None)); // Step support
                        placed.push((
                            p.x,
                            cy + 1,
                            p.z,
                            "minecraft:redstone_wire".to_string(),
                            None,
                        ));
                        cy += 1;
                    }
                }
            }
        } else {
            // Retry with a relaxed obstacle set: clear a 1-block neighborhood around
            // start and end. This lets the router carve a short tunnel through padding
            // when ports are only slightly embedded in obstacles.
            let mut relaxed = grid_obstacles.clone();
            for dx in -1..=1 {
                for dz in -1..=1 {
                    relaxed.remove(&(conn.src.x + dx, conn.src.z + dz));
                    relaxed.remove(&(conn.dst.x + dx, conn.dst.z + dz));
                }
            }

            if let Some(path) = find_path(conn.src, conn.dst, &relaxed) {
                let mut signal_dist = 0;
                for (idx, p) in path.iter().enumerate() {
                    let facing = if idx + 1 < path.len() {
                        let next = path[idx + 1];
                        if next.x > p.x {
                            "east"
                        } else if next.x < p.x {
                            "west"
                        } else if next.z > p.z {
                            "south"
                        } else {
                            "north"
                        }
                    } else {
                        "north"
                    };

                    // Mark and place
                    grid_obstacles.insert((p.x, p.z));
                    place_wire_fn(&mut placed, p.x, 1, p.z, &mut signal_dist, facing);
                }
            } else {
                // Final fallback: emit debug info and try a straight Manhattan carve
                eprintln!(
                    "Warning: No path found for connection {:?} -> {:?}",
                    conn.src, conn.dst
                );

                // Debug: print nearby obstacles
                let r = 3;
                eprintln!("Nearby obstacles around src:");
                for dz in -r..=r {
                    let mut line = String::new();
                    for dx in -r..=r {
                        let x = conn.src.x + dx;
                        let z = conn.src.z + dz;
                        line.push(if grid_obstacles.contains(&(x, z)) {
                            '#'
                        } else {
                            '.'
                        });
                    }
                    eprintln!("{}", line);
                }

                eprintln!("Nearby obstacles around dst:");
                for dz in -r..=r {
                    let mut line = String::new();
                    for dx in -r..=r {
                        let x = conn.dst.x + dx;
                        let z = conn.dst.z + dz;
                        line.push(if grid_obstacles.contains(&(x, z)) {
                            '#'
                        } else {
                            '.'
                        });
                    }
                    eprintln!("{}", line);
                }

                // Try straight Manhattan carve: go along X then Z
                let mut carve = Vec::new();
                let mut cx = conn.src.x;
                let mut cz = conn.src.z;
                while cx != conn.dst.x {
                    if conn.dst.x > cx {
                        cx += 1
                    } else {
                        cx -= 1
                    }
                    carve.push(Point { x: cx, z: cz });
                }
                while cz != conn.dst.z {
                    if conn.dst.z > cz {
                        cz += 1
                    } else {
                        cz -= 1
                    }
                    carve.push(Point { x: cx, z: cz });
                }

                if !carve.is_empty() {
                    let mut signal_dist = 0;
                    for (idx, p) in carve.iter().enumerate() {
                        let facing = if idx + 1 < carve.len() {
                            let next = carve[idx + 1];
                            if next.x > p.x {
                                "east"
                            } else if next.x < p.x {
                                "west"
                            } else if next.z > p.z {
                                "south"
                            } else {
                                "north"
                            }
                        } else {
                            "north"
                        };

                        // Remove obstacle and place
                        grid_obstacles.remove(&(p.x, p.z));
                        grid_obstacles.insert((p.x, p.z));
                        place_wire_fn(&mut placed, p.x, 1, p.z, &mut signal_dist, facing);
                    }
                }
            }
        }
    }

    // POST-PROCESSING: Calculate redstone wire connections
    // Redstone wire needs north/south/east/west properties to connect properly
    fn calculate_redstone_connections(
        placed: &mut Vec<(i32, i32, i32, String, Option<Vec<(String, String)>>)>,
    ) {
        // Build a map of block positions for quick lookup
        let mut block_map: HashMap<(i32, i32, i32), usize> = HashMap::new();
        for (idx, (x, y, z, _, _)) in placed.iter().enumerate() {
            block_map.insert((*x, *y, *z), idx);
        }

        // Check if a block can connect to redstone wire
        fn can_connect(name: &str) -> bool {
            name.contains("redstone")
                || name.contains("repeater")
                || name.contains("comparator")
                || name.contains("torch")
                || name == "minecraft:cobblestone"
                || name == "minecraft:sandstone"
        }

        // Update each redstone wire block
        for idx in 0..placed.len() {
            if placed[idx].3 == "minecraft:redstone_wire" {
                let (x, y, z, _, _) = placed[idx];
                let mut connections = Vec::new();

                // Check all four horizontal directions
                // North (-Z)
                let north_pos = (x, y, z - 1);
                let north_up = (x, y + 1, z - 1);
                let north_down = (x, y - 1, z - 1);

                if let Some(&north_idx) = block_map.get(&north_pos) {
                    if can_connect(&placed[north_idx].3) {
                        connections.push(("north".to_string(), "side".to_string()));
                    }
                } else if let Some(&north_up_idx) = block_map.get(&north_up) {
                    if can_connect(&placed[north_up_idx].3) {
                        connections.push(("north".to_string(), "up".to_string()));
                    }
                } else if let Some(&north_down_idx) = block_map.get(&north_down) {
                    if can_connect(&placed[north_down_idx].3) {
                        connections.push(("north".to_string(), "side".to_string()));
                    }
                } else {
                    connections.push(("north".to_string(), "none".to_string()));
                }

                // South (+Z)
                let south_pos = (x, y, z + 1);
                let south_up = (x, y + 1, z + 1);
                let south_down = (x, y - 1, z + 1);

                if let Some(&south_idx) = block_map.get(&south_pos) {
                    if can_connect(&placed[south_idx].3) {
                        connections.push(("south".to_string(), "side".to_string()));
                    }
                } else if let Some(&south_up_idx) = block_map.get(&south_up) {
                    if can_connect(&placed[south_up_idx].3) {
                        connections.push(("south".to_string(), "up".to_string()));
                    }
                } else if let Some(&south_down_idx) = block_map.get(&south_down) {
                    if can_connect(&placed[south_down_idx].3) {
                        connections.push(("south".to_string(), "side".to_string()));
                    }
                } else {
                    connections.push(("south".to_string(), "none".to_string()));
                }

                // East (+X)
                let east_pos = (x + 1, y, z);
                let east_up = (x + 1, y + 1, z);
                let east_down = (x + 1, y - 1, z);

                if let Some(&east_idx) = block_map.get(&east_pos) {
                    if can_connect(&placed[east_idx].3) {
                        connections.push(("east".to_string(), "side".to_string()));
                    }
                } else if let Some(&east_up_idx) = block_map.get(&east_up) {
                    if can_connect(&placed[east_up_idx].3) {
                        connections.push(("east".to_string(), "up".to_string()));
                    }
                } else if let Some(&east_down_idx) = block_map.get(&east_down) {
                    if can_connect(&placed[east_down_idx].3) {
                        connections.push(("east".to_string(), "side".to_string()));
                    }
                } else {
                    connections.push(("east".to_string(), "none".to_string()));
                }

                // West (-X)
                let west_pos = (x - 1, y, z);
                let west_up = (x - 1, y + 1, z);
                let west_down = (x - 1, y - 1, z);

                if let Some(&west_idx) = block_map.get(&west_pos) {
                    if can_connect(&placed[west_idx].3) {
                        connections.push(("west".to_string(), "side".to_string()));
                    }
                } else if let Some(&west_up_idx) = block_map.get(&west_up) {
                    if can_connect(&placed[west_up_idx].3) {
                        connections.push(("west".to_string(), "up".to_string()));
                    }
                } else if let Some(&west_down_idx) = block_map.get(&west_down) {
                    if can_connect(&placed[west_down_idx].3) {
                        connections.push(("west".to_string(), "side".to_string()));
                    }
                } else {
                    connections.push(("west".to_string(), "none".to_string()));
                }

                // Update the block with connection properties
                placed[idx].4 = Some(connections);
            }
        }
    }

    // Apply redstone wire connections
    calculate_redstone_connections(&mut placed);

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
    root_map.insert("MinecraftDataVersion".into(), Value::Int(4671)); // 1.16.5
    root_map.insert("Version".into(), Value::Int(7));

    let mut blob = nbt::Blob::new();
    for (k, v) in root_map {
        blob.insert(k, v)?;
    }

    let file = File::create(path)?;
    blob.to_gzip_writer(&mut std::io::BufWriter::new(file))?;
    Ok(())
}
