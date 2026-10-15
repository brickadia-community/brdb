use std::sync::Arc;

use crate::{
    IntVector,
    assets::LiteralComponent,
    schema::as_brdb::AsBrdbValue,
    wrapper::{BString, BrdbComponent},
};

pub const DYNAMIC_GRID: BString = BString::str("Entity_DynamicBrickGrid");
pub fn dynamic_grid_entity() -> Arc<Box<dyn BrdbComponent>> {
    Arc::new(Box::new(LiteralComponent::new(DYNAMIC_GRID)))
}

/// Game defaults for unset entity data fields whose zero value differs from
/// what the game writes for a fresh grid (zero would save an invisible,
/// massless, gravity-free grid). Entity data structs have no generated
/// `STRUCT_DEFAULTS` entry, so the writer falls back to this table before
/// writing a zero. Fields whose game default is zero are omitted.
pub(crate) fn entity_field_default(
    struct_name: &str,
    field: &str,
) -> Option<&'static dyn AsBrdbValue> {
    static TRUE: bool = true;
    static ONE: f32 = 1.0;
    static BUOYANCY: f32 = 0.2;
    match (struct_name, field) {
        ("BrickGridDynamicActor", "BouyancyScale") => Some(&BUOYANCY),
        ("BrickGridDynamicActor", "MassScale") => Some(&ONE),
        (
            "BrickGridDynamicActor",
            "bEnableGravity" | "bUseNewMassCalculation" | "bReceivesDecals" | "bVisible",
        ) => Some(&TRUE),
        _ => None,
    }
}

/// Entity surface type name (goes in `EntityTypeNames`) for the inner grid
/// of a microchip brick. Each microchip brick links to exactly one of these
/// via `ComponentChunkSoA`'s `microchip_brick_indices` /
/// `microchip_brick_grid_references`. Paired with
/// `MICROCHIP_GRID_CLASS` in `EntityDataClassNames`.
pub const MICROCHIP_GRID: BString = BString::str("Entity_MicrochipDynamicBrickGrid");
pub const MICROCHIP_GRID_CLASS: BString = BString::str("BP_MicrochipBrickGridDynamicActor_C");

/// Build the entity data for a microchip's inner grid.
///
/// `plane_extent` is half-size in grid units (matches the engine's
/// `BrickGridMicrochipActor.PlaneExtent` semantics). Default from in-game
/// placement is `(14, 14, 2)`.
pub fn microchip_grid_entity(
    collapsed: bool,
    plane_center: IntVector,
    plane_extent: IntVector,
) -> Arc<Box<dyn BrdbComponent>> {
    Arc::new(Box::new(LiteralComponent::new(MICROCHIP_GRID).with_data([
        ("bCollapsed", Box::new(collapsed) as Box<dyn AsBrdbValue>),
        ("PlaneCenter", Box::new(plane_center)),
        ("PlaneExtent", Box::new(plane_extent)),
    ])))
}
