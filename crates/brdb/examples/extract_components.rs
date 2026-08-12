//! Generate `src/assets/component_catalog.rs` — the ergonomic per-component
//! catalog (type name + host brick asset(s) + wire port names) — from a "zoo"
//! save: a world with every component placed and every wire port wired (built
//! in-game by the ue4ss inventory tool). Mirrors brs-js's COMPONENTS map.
//!
//! Usage:
//!   cargo run --example extract_components -- <zoo.brdb> src/assets/component_catalog.rs
//!
//! Regenerate whenever the game's components change (see the inventory pipeline
//! runbook). Host bricks come from each component's placed brick; ports come
//! from the zoo's wires (every port is wired), so both are complete only when
//! run against a fully-built, saved zoo.
use brdb::{AsBrdbValue, Brdb, IntoReader, WireChunkSoA};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let path = PathBuf::from(
        args.get(1)
            .expect("usage: extract_components <zoo.brdb> [out.rs]"),
    );
    let out_path = args.get(2).map(PathBuf::from);
    let db = Brdb::open(path)?.into_reader();
    let data = db.global_data()?;

    // The standalone phase attaches otherwise-brickless components to this generic host for
    // probing; it is never a real component host, so it is filtered out of the host lists.
    let standalone_host_brick = brdb::assets::bricks::B_1X1F_ROUND;
    let standalone_host: &str = standalone_host_brick.asset().as_ref();

    // component type index -> host brick asset name(s) / wire port indices.
    // Host names are resolved inline (basic and procedural bricks index different
    // name sets), so this stores names rather than type indices.
    let mut bricks: BTreeMap<u16, BTreeSet<String>> = BTreeMap::new();
    let mut inputs: BTreeMap<u16, BTreeSet<u16>> = BTreeMap::new();
    let mut outputs: BTreeMap<u16, BTreeSet<u16>> = BTreeMap::new();

    // Grid 1 is the main grid; higher ids are microchip inner grids. Probe
    // until a grid id is missing (same convention as read_components).
    for gid in 1..64 {
        let chunks = match db.brick_chunk_index(gid) {
            Ok(c) => c,
            Err(_) => break,
        };
        for chunk in &chunks {
            // brick index -> brick type index (basic-brick asset), for the
            // host-brick mapping.
            let bsoa = db.brick_chunk_soa(gid, chunk.index)?;
            let pb_start = bsoa.procedural_brick_starting_index;
            // Procedural brick type index (>= pb_start) -> procedural asset index, via the
            // per-size run-length counters (same expansion SoA::iter_bricks uses).
            let proc_asset_by_size: Vec<u32> = bsoa
                .brick_size_counters
                .iter()
                .flat_map(|c| std::iter::repeat(c.asset_index).take(c.num_sizes as usize))
                .collect();
            let brick_types = bsoa.brick_type_indices;

            if chunk.num_components > 0 {
                // component_chunk's Vec<BrdbStruct> only contains STRUCT-BEARING components
                // (it skips whole counters whose type has no data struct, e.g. some
                // Component_Internal_* gates), so its len() < the per-instance total. Looping
                // `0..components.len()` truncated the per-instance type/brick arrays, dropping
                // the host brick of any component in the tail (e.g. Component_Internal_InputSplitter).
                // Iterate the FULL per-instance count instead, matching brs-js's reader.
                let (csoa, _components) = db.component_chunk_soa(gid, chunk.index)?;
                let brick_indices = csoa.component_brick_indices;
                // Expand run-length (type_index, num_instances) into a flat
                // per-instance list of component type indices.
                let type_indices = csoa
                    .component_type_counters
                    .iter()
                    .flat_map(|v| {
                        let ti = v.type_index as u16;
                        (0..v.num_instances).map(move |_| ti)
                    })
                    .collect::<Vec<_>>();
                for i in 0..type_indices.len() {
                    let comp_ty = type_indices[i];
                    // ensure every placed component is present even with no wires
                    inputs.entry(comp_ty).or_default();
                    outputs.entry(comp_ty).or_default();
                    let brick_index = brick_indices[i].as_brdb_u32()? as usize;
                    if let Some(&bt) = brick_types.get(brick_index) {
                        // Basic bricks index basic_brick_asset_names directly; procedural
                        // bricks (type index >= pb_start) map through the size run-lengths to
                        // a procedural_brick_asset_names index.
                        let host = if bt < pb_start {
                            data.basic_brick_asset_names.get_index(bt as usize).cloned()
                        } else {
                            proc_asset_by_size
                                .get((bt - pb_start) as usize)
                                .and_then(|&ai| {
                                    data.procedural_brick_asset_names.get_index(ai as usize).cloned()
                                })
                        };
                        if let Some(host) = host {
                            if host != standalone_host {
                                bricks.entry(comp_ty).or_default().insert(host);
                            }
                        }
                    }
                }
            }

            if chunk.num_wires > 0 {
                let soa = db.wire_chunk_soa(gid, chunk.index)?.to_value();
                let soa: WireChunkSoA = (&soa).try_into()?;
                // local and remote ports are distinct types, so handle each in
                // its own loop (they share component_type_index / port_index).
                for p in &soa.local_wire_sources {
                    outputs
                        .entry(p.component_type_index)
                        .or_default()
                        .insert(p.port_index);
                }
                for p in &soa.remote_wire_sources {
                    outputs
                        .entry(p.component_type_index)
                        .or_default()
                        .insert(p.port_index);
                }
                for p in &soa.local_wire_targets {
                    inputs
                        .entry(p.component_type_index)
                        .or_default()
                        .insert(p.port_index);
                }
                for p in &soa.remote_wire_targets {
                    inputs
                        .entry(p.component_type_index)
                        .or_default()
                        .insert(p.port_index);
                }
            }
        }
    }

    // Resolve indices to names.
    let name_of = |idx: u16| data.component_type_names.get_index(idx as usize).cloned();
    let port_of = |idx: u16| data.component_wire_port_names.get_index(idx as usize).cloned();

    let all: BTreeSet<u16> = bricks
        .keys()
        .chain(inputs.keys())
        .chain(outputs.keys())
        .copied()
        .collect();

    // (name, host bricks, input ports, output ports), sorted by name.
    let mut rows: Vec<(String, Vec<String>, Vec<String>, Vec<String>)> = Vec::new();
    for ty in all {
        let Some(name) = name_of(ty) else { continue };
        let mut bs: Vec<String> = bricks
            .get(&ty)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        bs.sort();
        bs.dedup();
        let mut ins: Vec<String> = inputs
            .get(&ty)
            .map(|s| s.iter().filter_map(|&p| port_of(p)).collect())
            .unwrap_or_default();
        ins.sort();
        ins.dedup();
        let mut outs: Vec<String> = outputs
            .get(&ty)
            .map(|s| s.iter().filter_map(|&p| port_of(p)).collect())
            .unwrap_or_default();
        outs.sort();
        outs.dedup();
        rows.push((name, bs, ins, outs));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));

    let mut out = String::new();
    macro_rules! w {
        ($($t:tt)*) => { writeln!(out, $($t)*).unwrap() };
    }
    let slice = |v: &[String]| {
        v.iter()
            .map(|s| format!("{s:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    };

    w!("// Autogenerated from a zoo save:");
    w!("//   cargo run --example extract_components -- <zoo.brdb> src/assets/component_catalog.rs");
    w!("// Do not edit by hand.");
    w!();
    w!("/// Per-component catalog entry: the full component type name, its host");
    w!("/// brick asset(s), and its wire input/output port names. Extracted from a");
    w!("/// fully-placed, fully-wired \"zoo\" save (mirrors brs-js's COMPONENTS).");
    w!("#[derive(Debug, Clone, Copy, PartialEq, Eq)]");
    w!("pub struct ComponentInfo {{");
    w!("    /// e.g. \"BrickComponentType_WireGraph_Exec_Branch\".");
    w!("    pub name: &'static str,");
    w!("    /// Host brick asset name(s) that carry this component.");
    w!("    pub bricks: &'static [&'static str],");
    w!("    /// Wire input port names.");
    w!("    pub inputs: &'static [&'static str],");
    w!("    /// Wire output port names.");
    w!("    pub outputs: &'static [&'static str],");
    w!("}}");
    w!();
    w!("impl ComponentInfo {{");
    w!("    /// The primary host brick asset (first, if any).");
    w!("    pub const fn brick(&self) -> Option<&'static str> {{");
    w!("        self.bricks.first().copied()");
    w!("    }}");
    w!("}}");
    w!();
    w!(
        "/// Every component present in the zoo, sorted by `name` (binary-searchable)."
    );
    w!("pub static COMPONENTS: &[ComponentInfo] = &[");
    for (name, bs, ins, outs) in &rows {
        w!(
            "    ComponentInfo {{ name: {name:?}, bricks: &[{}], inputs: &[{}], outputs: &[{}] }},",
            slice(bs),
            slice(ins),
            slice(outs),
        );
    }
    w!("];");
    w!();
    w!("/// Look up a component by its full type name.");
    w!("pub fn component(name: &str) -> Option<&'static ComponentInfo> {{");
    w!("    COMPONENTS");
    w!("        .binary_search_by(|c| c.name.cmp(name))");
    w!("        .ok()");
    w!("        .map(|i| &COMPONENTS[i])");
    w!("}}");

    if let Some(ref p) = out_path {
        std::fs::write(p, &out)?;
        eprintln!("Wrote {}", p.display());
    } else {
        print!("{out}");
    }
    eprintln!(
        "Extracted {} components ({} with host bricks)",
        rows.len(),
        rows.iter().filter(|r| !r.1.is_empty()).count(),
    );
    Ok(())
}
