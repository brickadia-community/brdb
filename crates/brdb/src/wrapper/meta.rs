use serde::{Deserialize, Serialize};

/// Written into `Meta/Bundle.json` as `gameVersion`. A neutral placeholder so
/// generated bundles don't bake in a specific changelist. Migrations are driven
/// by the saved data/schema, not this field.
pub const GAME_VERSION: &str = "CL0";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BundleJson {
    #[serde(rename = "type")]
    pub level_type: String,
    #[serde(rename = "iD")]
    pub id: String,
    pub name: String,
    pub version: String,
    pub tags: Vec<String>,
    pub authors: Vec<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    pub description: String,
    // Unknown content
    pub dependencies: Vec<serde_json::Value>,
    #[serde(rename = "gameVersion")]
    pub game_version: String,
}

impl Default for BundleJson {
    fn default() -> Self {
        Self {
            level_type: "World".to_string(),
            id: "00000000-0000-0000-0000-000000000000".to_string(),
            name: "".to_string(),
            version: "".to_string(),
            tags: vec![],
            authors: vec![],
            created_at: "0001.01.01-00.00.00".to_string(),
            updated_at: "0001.01.01-00.00.00".to_string(),
            description: "A Generated World".to_string(),
            dependencies: vec![],
            game_version: GAME_VERSION.to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorldJson {
    pub environment: String,
}

impl Default for WorldJson {
    fn default() -> Self {
        Self {
            environment: "Plate".to_string(),
        }
    }
}

/// A float 3-vector as serialized in `Meta/Prefab.json` (pivot centers and
/// half-extents).
#[derive(Debug, Default, Clone, Copy, Deserialize, Serialize)]
pub struct PrefabVec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// An integer 3-vector as serialized in `Meta/Prefab.json`
/// (`addedGlobalGridOffset`).
#[derive(Debug, Default, Clone, Copy, Deserialize, Serialize)]
pub struct PrefabIntVec3 {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// A single pivot: a center point and an axis-aligned half-extent, both in
/// grid-local brick units.
#[derive(Debug, Default, Clone, Copy, Deserialize, Serialize)]
pub struct PrefabPivot {
    pub center: PrefabVec3,
    #[serde(rename = "halfExtent")]
    pub half_extent: PrefabVec3,
}

impl PrefabPivot {
    /// Build a pivot from inclusive min/max corners (in brick units).
    pub fn from_bounds(min: crate::wrapper::Position, max: crate::wrapper::Position) -> Self {
        Self {
            center: PrefabVec3 {
                x: (min.x + max.x) as f64 / 2.0,
                y: (min.y + max.y) as f64 / 2.0,
                z: (min.z + max.z) as f64 / 2.0,
            },
            half_extent: PrefabVec3 {
                x: (max.x - min.x) as f64 / 2.0,
                y: (max.y - min.y) as f64 / 2.0,
                z: (max.z - min.z) as f64 / 2.0,
            },
        }
    }
}

/// The `pivots` object of `Meta/Prefab.json`. The game uses these for preview
/// and stud-snapping when placing a prefab.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PrefabPivots {
    #[serde(rename = "bottomStudsPivot")]
    pub bottom_studs_pivot: PrefabPivot,
    #[serde(rename = "studsExpandedPivot")]
    pub studs_expanded_pivot: PrefabPivot,
    #[serde(rename = "topStudsPivot")]
    pub top_studs_pivot: PrefabPivot,
    #[serde(rename = "boundsPivot")]
    pub bounds_pivot: PrefabPivot,
    #[serde(rename = "bottomStudsDirection")]
    pub bottom_studs_direction: String,
    #[serde(rename = "topStudsDirection")]
    pub top_studs_direction: String,
    #[serde(rename = "bBottomStudsValid")]
    pub bottom_studs_valid: bool,
    #[serde(rename = "bTopStudsValid")]
    pub top_studs_valid: bool,
}

impl Default for PrefabPivots {
    fn default() -> Self {
        Self {
            bottom_studs_pivot: PrefabPivot::default(),
            studs_expanded_pivot: PrefabPivot::default(),
            top_studs_pivot: PrefabPivot::default(),
            bounds_pivot: PrefabPivot::default(),
            bottom_studs_direction: "Z_Negative".to_string(),
            top_studs_direction: "Z_Positive".to_string(),
            bottom_studs_valid: true,
            top_studs_valid: true,
        }
    }
}

/// `Meta/Prefab.json` — present only for `type: "Prefab"` bundles. Describes
/// the prefab's bounds/pivots and grid flags so the game can preview and place
/// it. Build one from a brick bounding box with [`PrefabJson::from_bounds`].
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PrefabJson {
    pub pivots: PrefabPivots,
    #[serde(rename = "addedGlobalGridOffset")]
    pub added_global_grid_offset: PrefabIntVec3,
    #[serde(rename = "bIsPhysicsGrid")]
    pub is_physics_grid: bool,
    #[serde(rename = "bFreezePhysicsGrid")]
    pub freeze_physics_grid: bool,
    #[serde(rename = "bFreezeGlobalGrid")]
    pub freeze_global_grid: bool,
    #[serde(rename = "bIsMicrochipPrefab")]
    pub is_microchip_prefab: bool,
}

impl Default for PrefabJson {
    fn default() -> Self {
        Self {
            pivots: PrefabPivots::default(),
            added_global_grid_offset: PrefabIntVec3::default(),
            is_physics_grid: false,
            freeze_physics_grid: false,
            freeze_global_grid: false,
            is_microchip_prefab: false,
        }
    }
}

impl PrefabJson {
    /// Build a prefab from an inclusive brick bounding box (in brick units).
    /// All four pivots are set to the bounds box; refine the stud pivots later
    /// if precise snapping is needed.
    pub fn from_bounds(min: crate::wrapper::Position, max: crate::wrapper::Position) -> Self {
        let pivot = PrefabPivot::from_bounds(min, max);
        Self {
            pivots: PrefabPivots {
                bottom_studs_pivot: pivot,
                studs_expanded_pivot: pivot,
                top_studs_pivot: pivot,
                bounds_pivot: pivot,
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct WorldMeta {
    /// Meta/Bundle.json
    pub bundle: BundleJson,
    /// Meta/Screenshot.jpg
    pub screenshot: Option<Vec<u8>>,
    /// Meta/Thumbnail.png
    pub thumbnail: Option<Vec<u8>>,
    /// Meta/World.json
    pub world: WorldJson,
    /// Meta/Prefab.json — written only when this is a prefab bundle.
    /// When `Some`, the write path emits a prefab (Bundle.json + Prefab.json,
    /// no World.json/Screenshot/Thumbnail).
    pub prefab: Option<PrefabJson>,
}
