mod literal_component;
pub use literal_component::*;
pub mod brick_sizes;
pub mod bricks;
pub mod component_catalog;
pub mod entities;
pub mod external;
pub mod gate_variants;
mod gates;
pub mod materials;

pub mod components {
    pub use super::component_catalog::{COMPONENTS, ComponentInfo, component};
    pub use super::gate_variants::{
        GateVariant, PROCEDURAL_GATES, ProceduralGate, procedural_gate,
        procedural_gate_for_component,
    };
    pub use super::gates::*;
    pub use super::literal_component::seat_component as seat;
}
