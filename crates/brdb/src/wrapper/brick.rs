use std::{
    cmp::Ordering,
    collections::HashMap,
    fmt::{Debug, Display},
    sync::Arc,
};

use crate::{
    BrdbSchemaError, IntVector, assets,
    schema::{
        BrdbSchemaGlobalData, BrdbValue,
        as_brdb::{AsBrdbIter, AsBrdbValue, BrdbArrayIter},
    },
    wrapper::{BString, BitFlags, BrdbComponent, Position},
};

pub struct Brick {
    /// An internal ID for linking bricks in the database.
    pub id: Option<usize>,
    pub asset: BrickType,
    pub owner_index: Option<usize>,
    /// Saved-world field added alongside the schema update. If `None`, serialization
    /// mirrors `owner_index` (preserving legacy behavior for Bricks constructed
    /// without explicitly setting an original owner).
    pub original_owner_index: Option<usize>,
    pub position: Position,
    pub rotation: Rotation,
    pub direction: Direction,
    pub collision: Collision,
    pub visible: bool,
    pub color: Color,
    pub material: BString,
    pub material_intensity: u8,
    pub components: Vec<Box<dyn BrdbComponent>>,
}

impl Brick {
    /// Monotonic id counter used for both bricks and entities. The write
    /// path keeps separate maps (`brick_id_map`, `entity_index_map`) so
    /// ids minted here serve both without collision concerns.
    pub fn next_id() -> usize {
        static NEXT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    /// Returns the ID of the brick if it has one.
    pub fn get_id(&self) -> Option<usize> {
        self.id
    }

    /// Sets the ID of the brick to a new value if it does not already have one.
    pub fn with_id(mut self) -> Self {
        if self.id.is_some() {
            return self;
        }
        self.id = Some(Self::next_id());
        self
    }

    /// Sets the ID of the brick to a new value if it does not already have one.
    pub fn with_id_split(mut self) -> (Self, usize) {
        if let Some(id) = self.id {
            return (self, id);
        }
        let id = Self::next_id();
        self.id = Some(id);
        (self, id)
    }

    /// Adds an ID to the brick if it does not already have one.
    pub fn add_id(&mut self) -> usize {
        if let Some(id) = self.id {
            return id;
        }
        let id = Self::next_id();
        self.id = Some(id);
        id
    }

    /// True if this brick's asset is the outer microchip shell. The microchip's
    /// inner grid is held in a separate `Entity` (see
    /// `assets::entities::MICROCHIP_GRID`); the link is recorded at save time
    /// in `ComponentChunkSoA`'s `microchip_brick_indices` /
    /// `microchip_brick_grid_references`. Use `World::add_microchip` to emit
    /// a linked (brick, entity) pair in one call.
    pub fn is_microchip_host(&self) -> bool {
        self.asset == assets::bricks::B_MICROCHIP
    }

    /// Adds a component to the brick. The component must implement the `BrdbComponent` trait.
    pub fn add_component(&mut self, component: impl BrdbComponent + 'static) {
        self.components.push(Box::new(component));
    }
    /// Adds a component to the brick. The component must implement the `BrdbComponent` trait.
    pub fn add_component_box(&mut self, component: Box<dyn BrdbComponent>) {
        self.components.push(component);
    }
    /// Adds multiple components to the brick. The components must implement the `BrdbComponent` trait.
    pub fn add_components(&mut self, components: impl IntoIterator<Item = Box<dyn BrdbComponent>>) {
        self.components.extend(components);
    }
    /// Adds a component to the brick. The component must implement the `BrdbComponent` trait.
    pub fn with_component(mut self, component: impl BrdbComponent + 'static) -> Self {
        self.add_component(component);
        self
    }
    /// Adds a component to the brick. The component must implement the `BrdbComponent` trait.
    pub fn with_component_box(mut self, component: Box<dyn BrdbComponent>) -> Self {
        self.add_component_box(component);
        self
    }
    /// Adds multiple components to the brick. The components must implement the `BrdbComponent` trait.
    pub fn with_components(
        mut self,
        components: impl IntoIterator<Item = Box<dyn BrdbComponent>>,
    ) -> Self {
        self.add_components(components);
        self
    }

    pub fn cmp(&self, other: &Self) -> Ordering {
        match self.asset.cmp(&other.asset) {
            Ordering::Equal => self.position.cmp(&other.position),
            ord => ord,
        }
    }

    /// Sets the material of the brick.
    pub fn set_material(&mut self, material: impl Into<BString>) {
        self.material = material.into();
    }
    /// Sets the material of the brick.
    pub fn with_material(mut self, material: impl Into<BString>) -> Self {
        self.set_material(material);
        self
    }

    /// Axis-aligned bounds of this brick in grid-local brick units, as
    /// `(min, max)` corners. Procedural bricks use their half-extent `size`;
    /// basic bricks (whose extent is asset-defined and not carried here) are
    /// treated as a point at their position. Brick rotation is not applied —
    /// the extent is taken as authored, which is exact for unrotated bricks
    /// and an approximation otherwise.
    pub fn local_bounds(&self) -> (Position, Position) {
        let half = match &self.asset {
            BrickType::Procedural { size, .. } => Position {
                x: size.x as i32,
                y: size.y as i32,
                z: size.z as i32,
            },
            // The collapsed microchip shell is a 1x1 plate footprint (matches
            // what the game writes for a microchip prefab: half-extent 5,5,2).
            BrickType::Basic(asset) if asset.as_ref() == "B_1x1_Microchip" => {
                Position { x: 5, y: 5, z: 2 }
            }
            // The reroute node is a small 2x2x2 node (half-extent 1,1,1). Its
            // real size matters for prefab bounds: the generic 5,5,6 default
            // below would push a rerouter's bounds far past the brick and, for
            // one sitting beside a microchip, float the whole pasted prefab.
            BrickType::Basic(asset) if asset.as_ref() == "B_1x1_Reroute_Node" => {
                Position { x: 1, y: 1, z: 1 }
            }
            // Other basic bricks carry no size here; assume a 1x1 brick
            // footprint rather than a zero-size point so prefab bounds are sane.
            BrickType::Basic(_) => Position { x: 5, y: 5, z: 6 },
        };
        (self.position - half, self.position + half)
    }
}

impl Default for Brick {
    fn default() -> Self {
        Self {
            id: None,
            asset: BrickType::Procedural {
                asset: assets::bricks::PB_DEFAULT_BRICK,
                size: BrickSize { x: 5, y: 5, z: 6 },
            },
            owner_index: None,
            original_owner_index: None,
            position: Position { x: 0, y: 0, z: 0 },
            rotation: Default::default(),
            direction: Default::default(),
            collision: Default::default(),
            visible: true,
            color: Default::default(),
            material_intensity: 5,
            material: assets::materials::PLASTIC,
            components: Default::default(),
        }
    }
}

impl Clone for Brick {
    fn clone(&self) -> Self {
        Self {
            id: None, // IDs are not cloned, they are unique per brick
            asset: self.asset.clone(),
            owner_index: self.owner_index.clone(),
            original_owner_index: self.original_owner_index.clone(),
            position: self.position.clone(),
            rotation: self.rotation.clone(),
            direction: self.direction.clone(),
            collision: self.collision.clone(),
            visible: self.visible.clone(),
            color: self.color.clone(),
            material: self.material.clone(),
            material_intensity: self.material_intensity.clone(),
            components: self
                .components
                .iter()
                // See `BoxedComponent` why this is necessary...
                .map(|c| c.boxed_component())
                .collect(),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct Collision {
    pub player: bool,
    pub player1: Option<bool>,
    pub player2: Option<bool>,
    pub player3: Option<bool>,
    pub weapon: bool,
    pub interact: bool,
    pub tool: bool,
    pub physics: bool,
}

impl Default for Collision {
    fn default() -> Self {
        Self {
            player: true,
            player1: None,
            player2: None,
            player3: None,
            weapon: true,
            interact: true,
            tool: true,
            physics: true,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}
impl Color {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
    pub fn monochrome(value: u8) -> Self {
        Self {
            r: value,
            g: value,
            b: value,
        }
    }

    /// Convert HSV to RGB
    pub fn hsv(hue: f32, saturation: f32, value: f32) -> Self {
        let c = value * saturation;
        let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
        let m = value - c;

        let (r, g, b) = if hue < 60.0 {
            (c, x, 0.0)
        } else if hue < 120.0 {
            (x, c, 0.0)
        } else if hue < 180.0 {
            (0.0, c, x)
        } else if hue < 240.0 {
            (0.0, x, c)
        } else if hue < 300.0 {
            (x, 0.0, c)
        } else {
            (c, 0.0, x)
        };

        Self {
            r: ((r + m) * 255.0) as u8,
            g: ((g + m) * 255.0) as u8,
            b: ((b + m) * 255.0) as u8,
        }
    }

    /// Convert from srgb to linear
    pub fn to_linear(self) -> Self {
        // Convert sRGB to linear RGB
        let r = if self.r <= 0x0F {
            (self.r as f32 / 15.0).powf(2.2) * 255.0
        } else {
            (self.r as f32 / 255.0).powf(2.2) * 255.0
        } as u8;
        let g = if self.g <= 0x0F {
            (self.g as f32 / 15.0).powf(2.2) * 255.0
        } else {
            (self.g as f32 / 255.0).powf(2.2) * 255.0
        } as u8;
        let b = if self.b <= 0x0F {
            (self.b as f32 / 15.0).powf(2.2) * 255.0
        } else {
            (self.b as f32 / 255.0).powf(2.2) * 255.0
        } as u8;
        Self { r, g, b }
    }

    /// Convert from Linear RGB to sRGB
    pub fn to_srgb(self) -> Self {
        // Convert linear RGB to sRGB
        let r = if self.r <= 0x0F {
            (self.r as f32 / 255.0).powf(1.0 / 2.2) * 15.0
        } else {
            (self.r as f32 / 255.0).powf(1.0 / 2.2) * 255.0
        } as u8;
        let g = if self.g <= 0x0F {
            (self.g as f32 / 255.0).powf(1.0 / 2.2) * 15.0
        } else {
            (self.g as f32 / 255.0).powf(1.0 / 2.2) * 255.0
        } as u8;
        let b = if self.b <= 0x0F {
            (self.b as f32 / 255.0).powf(1.0 / 2.2) * 15.0
        } else {
            (self.b as f32 / 255.0).powf(1.0 / 2.2) * 255.0
        } as u8;
        Self { r, g, b }
    }
}
impl From<(u8, u8, u8)> for Color {
    fn from((r, g, b): (u8, u8, u8)) -> Self {
        Self { r, g, b }
    }
}

impl Default for Color {
    fn default() -> Self {
        Self {
            r: 255,
            g: 255,
            b: 255,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SavedBrickColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl SavedBrickColor {
    pub fn new(color: Color, alpha: u8) -> Self {
        Self {
            r: color.r,
            g: color.g,
            b: color.b,
            a: alpha,
        }
    }
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    #[inline]
    pub const fn color(&self) -> Color {
        Color {
            r: self.r,
            g: self.g,
            b: self.b,
        }
    }

    pub const fn entity_default() -> Self {
        Self {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        }
    }

    /// Convert this color's RGB from linear to sRGB, leaving `a` (material
    /// intensity / alpha) untouched. Used when reading saves whose colors are
    /// stored linear (`bColorsAreLinear`); in-memory colors are kept sRGB.
    pub fn rgb_to_srgb(self) -> Self {
        let c = self.color().to_srgb();
        Self {
            r: c.r,
            g: c.g,
            b: c.b,
            a: self.a,
        }
    }
}

impl From<Color> for SavedBrickColor {
    fn from(color: Color) -> Self {
        Self::rgb(color.r, color.g, color.b)
    }
}

impl Default for SavedBrickColor {
    fn default() -> Self {
        Self {
            r: 255,
            g: 255,
            b: 255,
            a: 5,
        }
    }
}

impl AsBrdbValue for SavedBrickColor {
    fn as_brdb_struct_prop_value(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, crate::errors::BrdbSchemaError> {
        let field = prop_name.get(schema).unwrap();
        match field {
            "R" => Ok(&self.r),
            "G" => Ok(&self.g),
            "B" => Ok(&self.b),
            "A" => Ok(&self.a),
            n => unimplemented!("unimplemented struct field {n}"),
        }
    }
}

impl TryFrom<&BrdbValue> for SavedBrickColor {
    type Error = crate::errors::BrdbSchemaError;

    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        Ok(Self {
            r: value.prop("R")?.as_brdb_u8()?,
            g: value.prop("G")?.as_brdb_u8()?,
            b: value.prop("B")?.as_brdb_u8()?,
            a: value.prop("A")?.as_brdb_u8()?,
        })
    }
}

impl TryFrom<BrdbValue> for SavedBrickColor {
    type Error = crate::errors::BrdbSchemaError;

    fn try_from(value: BrdbValue) -> Result<Self, Self::Error> {
        Ok(Self {
            r: value.prop("R")?.as_brdb_u8()?,
            g: value.prop("G")?.as_brdb_u8()?,
            b: value.prop("B")?.as_brdb_u8()?,
            a: value.prop("A")?.as_brdb_u8()?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrickType {
    Basic(BString),
    Procedural { asset: BString, size: BrickSize },
}

impl BrickType {
    pub const fn str(asset: &'static str) -> Self {
        BrickType::Basic(BString::str(asset))
    }
}

impl BrickType {
    pub fn is_procedural(&self) -> bool {
        matches!(self, BrickType::Procedural { .. })
    }

    pub fn is_basic(&self) -> bool {
        matches!(self, BrickType::Basic(_))
    }

    pub fn asset(&self) -> &BString {
        match self {
            BrickType::Basic(asset) => asset,
            BrickType::Procedural { asset, .. } => asset,
        }
    }
}

impl<T: Into<BString>> From<T> for BrickType {
    fn from(asset: T) -> Self {
        BrickType::Basic(asset.into())
    }
}

impl<T: Into<BString>, B: Into<BrickSize>> From<(T, B)> for BrickType {
    fn from((asset, size): (T, B)) -> Self {
        BrickType::Procedural {
            asset: asset.into(),
            size: size.into(),
        }
    }
}

impl Ord for BrickType {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (BrickType::Basic(a), BrickType::Basic(b)) => a.cmp(b),
            // Basic bricks sort ascending before procedural bricks
            (BrickType::Basic(_), BrickType::Procedural { .. }) => Ordering::Less,
            // Procedural bricks are always greater than basic bricks
            (BrickType::Procedural { .. }, BrickType::Basic(_)) => Ordering::Greater,
            (
                BrickType::Procedural {
                    asset: a,
                    size: a_size,
                },
                BrickType::Procedural {
                    asset: b,
                    size: b_size,
                },
            ) => match a.cmp(b) {
                Ordering::Equal => a_size.cmp(b_size),
                ord => ord,
            },
        }
    }
}

impl PartialOrd for BrickType {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub const CHUNK_SIZE: i32 = 2048;
pub const CHUNK_HALF: i32 = CHUNK_SIZE / 2;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ChunkIndex {
    pub x: i16,
    pub y: i16,
    pub z: i16,
}
impl ChunkIndex {
    pub const ZERO: Self = Self { x: 0, y: 0, z: 0 };
    pub const fn new(x: i16, y: i16, z: i16) -> Self {
        Self { x, y, z }
    }
}
impl From<(i16, i16, i16)> for ChunkIndex {
    fn from((x, y, z): (i16, i16, i16)) -> Self {
        Self { x, y, z }
    }
}
impl AsBrdbValue for ChunkIndex {
    fn as_brdb_struct_prop_value(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, crate::errors::BrdbSchemaError> {
        let field = prop_name.get(schema).unwrap();
        match field {
            "X" => Ok(&self.x),
            "Y" => Ok(&self.y),
            "Z" => Ok(&self.z),
            n => unimplemented!("unimplemented struct field {n}"),
        }
    }
}
impl TryFrom<&BrdbValue> for ChunkIndex {
    type Error = crate::errors::BrdbSchemaError;

    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        Ok(Self {
            x: value.prop("X")?.as_brdb_i16()?,
            y: value.prop("Y")?.as_brdb_i16()?,
            z: value.prop("Z")?.as_brdb_i16()?,
        })
    }
}

impl TryFrom<BrdbValue> for ChunkIndex {
    type Error = crate::errors::BrdbSchemaError;

    fn try_from(value: BrdbValue) -> Result<Self, Self::Error> {
        Ok(Self {
            x: value.prop("X")?.as_brdb_i16()?,
            y: value.prop("Y")?.as_brdb_i16()?,
            z: value.prop("Z")?.as_brdb_i16()?,
        })
    }
}

impl Display for ChunkIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}_{}_{}", self.x, self.y, self.z)
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct BrickSize {
    pub x: u16,
    pub y: u16,
    pub z: u16,
}
impl BrickSize {
    pub const fn new(x: u16, y: u16, z: u16) -> Self {
        Self { x, y, z }
    }
}
impl From<(u16, u16, u16)> for BrickSize {
    fn from((x, y, z): (u16, u16, u16)) -> Self {
        Self { x, y, z }
    }
}
impl From<BrickSize> for Position {
    fn from(size: BrickSize) -> Self {
        Position {
            x: size.x as i32,
            y: size.y as i32,
            z: size.z as i32,
        }
    }
}

impl TryFrom<&BrdbValue> for BrickSize {
    type Error = crate::errors::BrdbSchemaError;

    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        Ok(Self {
            x: value.prop("X")?.as_brdb_u16()?,
            y: value.prop("Y")?.as_brdb_u16()?,
            z: value.prop("Z")?.as_brdb_u16()?,
        })
    }
}

impl TryFrom<BrdbValue> for BrickSize {
    type Error = crate::errors::BrdbSchemaError;

    fn try_from(value: BrdbValue) -> Result<Self, Self::Error> {
        Ok(Self {
            x: value.prop("X")?.as_brdb_u16()?,
            y: value.prop("Y")?.as_brdb_u16()?,
            z: value.prop("Z")?.as_brdb_u16()?,
        })
    }
}

impl Ord for BrickSize {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.z.cmp(&other.z) {
            Ordering::Equal => match self.y.cmp(&other.y) {
                Ordering::Equal => self.x.cmp(&other.x),
                ord => ord,
            },
            ord => ord,
        }
    }
}

impl PartialOrd for BrickSize {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl AsBrdbValue for BrickSize {
    fn as_brdb_struct_prop_value(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, crate::errors::BrdbSchemaError> {
        let field = prop_name.get(schema).unwrap();
        match field {
            "X" => Ok(&self.x),
            "Y" => Ok(&self.y),
            "Z" => Ok(&self.z),
            n => unimplemented!("unimplemented struct field {n}"),
        }
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct RelativePosition {
    pub x: i16,
    pub y: i16,
    pub z: i16,
}

impl AsBrdbValue for RelativePosition {
    fn as_brdb_struct_prop_value(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, crate::errors::BrdbSchemaError> {
        let field = prop_name.get(schema).unwrap();
        match field {
            "X" => Ok(&self.x),
            "Y" => Ok(&self.y),
            "Z" => Ok(&self.z),
            n => unimplemented!("unimplemented struct field {n}"),
        }
    }
}

impl TryFrom<&BrdbValue> for RelativePosition {
    type Error = crate::errors::BrdbSchemaError;

    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        Ok(Self {
            x: value.prop("X")?.as_brdb_i16()?,
            y: value.prop("Y")?.as_brdb_i16()?,
            z: value.prop("Z")?.as_brdb_i16()?,
        })
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Default)]
pub enum Direction {
    XPositive,
    XNegative,
    YPositive,
    YNegative,
    #[default]
    ZPositive,
    ZNegative,
    MAX,
}

impl AsBrdbValue for Direction {
    fn as_brdb_enum(
        &self,
        _schema: &crate::schema::BrdbSchema,
        _def: &crate::schema::BrdbSchemaEnum,
    ) -> Result<i32, crate::errors::BrdbSchemaError> {
        Ok((*self as u8) as i32)
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Default)]
pub enum Rotation {
    #[default]
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

pub fn orientation_to_byte(dir: Direction, rot: Rotation) -> u8 {
    (dir as u8) << 2 | rot as u8
}

pub fn byte_to_orientation(orientation: u8) -> (Direction, Rotation) {
    let dir = match (orientation >> 2) % 6 {
        0 => Direction::XPositive,
        1 => Direction::XNegative,
        2 => Direction::YPositive,
        3 => Direction::YNegative,
        4 => Direction::ZPositive,
        _ => Direction::ZNegative,
    };
    let rot = match orientation & 3 {
        0 => Rotation::Deg0,
        1 => Rotation::Deg90,
        2 => Rotation::Deg180,
        _ => Rotation::Deg270,
    };
    (dir, rot)
}

#[derive(Clone, Debug)]
pub struct BrickSizeCounter {
    pub asset_index: u32,
    pub num_sizes: u32,
}

impl AsBrdbValue for BrickSizeCounter {
    fn as_brdb_struct_prop_value(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, crate::errors::BrdbSchemaError> {
        let field = prop_name.get(schema).unwrap();
        match field {
            "AssetIndex" => Ok(&self.asset_index),
            "NumSizes" => Ok(&self.num_sizes),
            n => unimplemented!("unimplemented struct field {n}"),
        }
    }
}

impl TryFrom<&BrdbValue> for BrickSizeCounter {
    type Error = crate::errors::BrdbSchemaError;

    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        Ok(Self {
            asset_index: value.prop("AssetIndex")?.as_brdb_u32()?,
            num_sizes: value.prop("NumSizes")?.as_brdb_u32()?,
        })
    }
}

impl TryFrom<BrdbValue> for BrickSizeCounter {
    type Error = crate::errors::BrdbSchemaError;

    fn try_from(value: BrdbValue) -> Result<Self, Self::Error> {
        Ok(Self {
            asset_index: value.prop("AssetIndex")?.as_brdb_u32()?,
            num_sizes: value.prop("NumSizes")?.as_brdb_u32()?,
        })
    }
}

#[derive(Default, Debug)]
pub struct BrickChunkSoA {
    /// The number of basic bricks at the time of saving this chunk.
    pub procedural_brick_starting_index: u32,
    pub brick_size_counters: Vec<BrickSizeCounter>,
    pub brick_sizes: Vec<BrickSize>,
    pub brick_type_indices: Vec<u32>,

    pub owner_indices: Vec<u32>,
    pub original_owner_indices: Vec<u32>,

    pub relative_positions: Vec<RelativePosition>,
    pub orientations: Vec<u8>,
    pub collision_flags_player: BitFlags,
    pub collision_flags_player1: BitFlags,
    pub collision_flags_player2: BitFlags,
    pub collision_flags_player3: BitFlags,
    pub collision_flags_weapon: BitFlags,
    pub collision_flags_interaction: BitFlags,
    pub collision_flags_physics: BitFlags,
    pub visibility_flags: BitFlags,
    pub material_indices: Vec<u8>,
    // RGB + Material intensity
    pub colors_and_alphas: Vec<SavedBrickColor>,
    // A map of (asset_index, size) to the index in the brick_sizes vector
    size_index_map: HashMap<(u32, BrickSize), u32>,
    // The number of procedural brick sizes
    num_brick_sizes: u32,
}

impl BrickChunkSoA {
    /// Add a brick to the chunk. Brick asset types cannot change after adding the first brick or
    /// it will break the world
    pub fn add_brick(&mut self, global_data: &BrdbSchemaGlobalData, brick: &Brick) {
        use BrickType::*;

        // Handle adding the asset type first
        match &brick.asset {
            Basic(asset) => {
                // Unwrap safety: The brick meta is added to the global data before adding bricks.
                let ty_index = global_data
                    .basic_brick_asset_names
                    .get_index_of(asset.as_ref())
                    .unwrap() as u32;
                self.brick_type_indices.push(ty_index);
            }
            Procedural { asset, size } => {
                // Unwrap safety: The brick meta is added to the global data before adding bricks.
                let ty_index = global_data
                    .procedural_brick_asset_names
                    .get_index_of(asset.as_ref())
                    .unwrap() as u32;

                let size_index =
                // Check to see if this size and asset pair already exists
                    if let Some(size_index) = self.size_index_map.get(&(ty_index, *size)) {
                        *size_index
                    } else {
                        // The new size index is based how many size/asset pairs after the number of basic bricks
                        let size_index =
                            self.num_brick_sizes + global_data.basic_brick_asset_names.len() as u32;

                        'size: {
                            // If the last entry has the same asset index...
                            if let (Some(last_sizes), Some(last_size)) = (self.brick_size_counters.last_mut(), self.brick_sizes.last())
                                // Check if the last asset and size match the current one
                                && last_sizes.asset_index == ty_index
                            {
                                if last_size != size {
                                    // Increment the size count for the last asset
                                    last_sizes.num_sizes += 1;
                                } else {
                                    break 'size;
                                }
                            } else {
                                // Otherwise, add a new size/asset pair counter
                                self.brick_size_counters.push(BrickSizeCounter {
                                    asset_index: ty_index,
                                    num_sizes: 1,
                                });
                            }

                            // Add the new size and increment the size index map
                            self.brick_sizes.push(*size);
                            self.size_index_map.insert((ty_index, *size), size_index);
                            self.num_brick_sizes += 1;
                        }


                        size_index
                    };

                self.brick_type_indices.push(size_index);
            }
        }

        self.owner_indices
            .push(brick.owner_index.unwrap_or(0) as u32);
        // If original_owner_index is unset, mirror the current owner (legacy behavior).
        self.original_owner_indices.push(
            brick
                .original_owner_index
                .or(brick.owner_index)
                .unwrap_or(0) as u32,
        );

        self.relative_positions.push(brick.position.to_relative().1);
        self.orientations
            .push(orientation_to_byte(brick.direction, brick.rotation));

        self.collision_flags_player.push(brick.collision.player);
        self.collision_flags_player1
            .push(brick.collision.player1.unwrap_or(brick.collision.player));
        self.collision_flags_player2
            .push(brick.collision.player2.unwrap_or(brick.collision.player));
        self.collision_flags_player3
            .push(brick.collision.player3.unwrap_or(brick.collision.player));
        self.collision_flags_weapon.push(brick.collision.weapon);
        self.collision_flags_interaction
            .push(brick.collision.interact);
        // CollisionFlags_Tool was removed from the schema; Collision::tool
        // stays in the public struct for backward-compatible callers but
        // is no longer serialized.
        self.collision_flags_physics.push(brick.collision.physics);
        self.visibility_flags.push(brick.visible);

        self.material_indices.push(
            global_data
                .material_asset_names
                .get_index_of(brick.material.as_ref())
                .unwrap() as u8, // Unwrap safety: The material is added to the global data before adding bricks.
        );

        self.colors_and_alphas
            .push(SavedBrickColor::new(brick.color, brick.material_intensity));
    }

    /// Convert the SoA into an iterator of bricks.
    /// The `chunk_index` is required to convert relative positions to absolute positions.
    /// The `global_data` is required to look up asset names by index.
    pub fn iter_bricks(
        &self,
        chunk_index: ChunkIndex,
        global_data: Arc<BrdbSchemaGlobalData>,
    ) -> impl Iterator<Item = Result<Brick, BrdbSchemaError>> {
        let proc_brick_index = self.procedural_brick_starting_index as usize;

        // Zip the brick size counters with the brick sizes
        let proc_brick_sizes = self
            .brick_sizes
            .iter()
            .copied()
            .zip(
                // repeat the asset index based on the number of sizes for each asset
                self.brick_size_counters
                    .iter()
                    .flat_map(|c| (0..c.num_sizes).map(|_| c.asset_index)),
            )
            .collect::<Vec<_>>();

        self.brick_type_indices
            .iter()
            .enumerate()
            .map(move |(i, &ty_index)| {
                let ty_index = ty_index as usize;

                let asset = if ty_index < proc_brick_index {
                    BrickType::Basic(global_data.basic_brick_asset_by_index(ty_index)?)
                } else {
                    // Lookup the procedural brick size by an index offset by the number of basic brick types
                    let size_index = ty_index.saturating_sub(proc_brick_index);
                    let (size, asset_index) =
                        proc_brick_sizes.get(size_index).ok_or_else(|| {
                            BrdbSchemaError::Wrapped(
                                "Procedural brick with index".to_string(),
                                Box::new(BrdbSchemaError::ArrayIndexOutOfBounds {
                                    index: size_index,
                                    len: self.brick_sizes.len(),
                                }),
                            )
                        })?;

                    let asset =
                        global_data.procedural_brick_asset_by_index(*asset_index as usize)?;
                    BrickType::Procedural { asset, size: *size }
                };

                let position = Position::from_relative(chunk_index, self.relative_positions[i]);
                let (direction, rotation) = byte_to_orientation(self.orientations[i]);
                let color = self.colors_and_alphas[i];
                Ok(Brick {
                    id: None,
                    asset,
                    position,
                    direction,
                    rotation,
                    collision: Collision {
                        player: self.collision_flags_player.get(i),
                        player1: Some(self.collision_flags_player1.get(i)),
                        player2: Some(self.collision_flags_player2.get(i)),
                        player3: Some(self.collision_flags_player3.get(i)),
                        weapon: self.collision_flags_weapon.get(i),
                        interact: self.collision_flags_interaction.get(i),
                        // Deprecated — see Collision::tool comment.
                        tool: true,
                        physics: self.collision_flags_physics.get(i),
                    },
                    visible: self.visibility_flags.get(i),
                    owner_index: Some(self.owner_indices[i] as usize),
                    original_owner_index: self
                        .original_owner_indices
                        .get(i)
                        .map(|v| *v as usize),
                    color: color.color(),
                    material: global_data
                        .material_asset_by_index(self.material_indices[i] as usize)?,
                    material_intensity: color.a,
                    components: Vec::new(), // Components are not stored in the brick chunk
                })
            })
    }
}

impl AsBrdbValue for BrickChunkSoA {
    fn as_brdb_struct_prop_value(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, crate::errors::BrdbSchemaError> {
        match prop_name.get(schema).unwrap() {
            "ProceduralBrickStartingIndex" => Ok(&self.procedural_brick_starting_index),
            "CollisionFlags_Player" => Ok(&self.collision_flags_player),
            "CollisionFlags_Player1" => Ok(&self.collision_flags_player1),
            "CollisionFlags_Player2" => Ok(&self.collision_flags_player2),
            "CollisionFlags_Player3" => Ok(&self.collision_flags_player3),
            "CollisionFlags_Weapon" => Ok(&self.collision_flags_weapon),
            "CollisionFlags_Interaction" => Ok(&self.collision_flags_interaction),
            "CollisionFlags_Physics" => Ok(&self.collision_flags_physics),
            "VisibilityFlags" => Ok(&self.visibility_flags),
            // New saves always store sRGB colors, so this is always false.
            "bColorsAreLinear" => Ok(&false),
            n => unimplemented!("unimplemented struct field {n}"),
        }
    }

    fn as_brdb_struct_prop_array(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> Result<BrdbArrayIter<'_>, crate::errors::BrdbSchemaError> {
        match prop_name.get(schema).unwrap() {
            "BrickSizeCounters" => Ok(self.brick_size_counters.as_brdb_iter()),
            "BrickSizes" => Ok(self.brick_sizes.as_brdb_iter()),
            "BrickTypeIndices" => Ok(self.brick_type_indices.as_brdb_iter()),
            "OwnerIndices" => Ok(self.owner_indices.as_brdb_iter()),
            "OriginalOwnerIndices" => Ok(self.original_owner_indices.as_brdb_iter()),
            "RelativePositions" => Ok(self.relative_positions.as_brdb_iter()),
            "Orientations" => Ok(self.orientations.as_brdb_iter()),
            "MaterialIndices" => Ok(self.material_indices.as_brdb_iter()),
            "ColorsAndAlphas" => Ok(self.colors_and_alphas.as_brdb_iter()),
            n => unimplemented!("unimplemented struct field {n}"),
        }
    }
}

impl TryFrom<&BrdbValue> for BrickChunkSoA {
    type Error = crate::errors::BrdbSchemaError;

    fn try_from(value: &BrdbValue) -> Result<Self, Self::Error> {
        let collision_flags_player: BitFlags = value.prop("CollisionFlags_Player")?.try_into()?;
        let mut base = BrickChunkSoA {
            procedural_brick_starting_index: value
                .prop("ProceduralBrickStartingIndex")?
                .as_brdb_u32()?,
            brick_size_counters: value.prop("BrickSizeCounters")?.try_into()?,
            brick_sizes: value.prop("BrickSizes")?.try_into()?,
            brick_type_indices: value.prop("BrickTypeIndices")?.try_into()?,
            owner_indices: value.prop("OwnerIndices")?.try_into()?,
            // Added alongside the schema update for per-brick original-owner tracking.
            // Older saves won't have this field; mirror OwnerIndices so round-trips
            // through the SoA stay consistent.
            original_owner_indices: if value.contains_key("OriginalOwnerIndices") {
                value.prop("OriginalOwnerIndices")?.try_into()?
            } else {
                value.prop("OwnerIndices")?.try_into()?
            },
            relative_positions: value.prop("RelativePositions")?.try_into()?,
            orientations: value.prop("Orientations")?.try_into()?,

            // Handle migration for per-player collision flags
            collision_flags_player1: if value.contains_key("CollisionFlags_Player1") {
                value.prop("CollisionFlags_Player1")?.try_into()?
            } else {
                collision_flags_player.clone()
            },
            collision_flags_player2: if value.contains_key("CollisionFlags_Player2") {
                value.prop("CollisionFlags_Player2")?.try_into()?
            } else {
                collision_flags_player.clone()
            },
            collision_flags_player3: if value.contains_key("CollisionFlags_Player3") {
                value.prop("CollisionFlags_Player3")?.try_into()?
            } else {
                collision_flags_player.clone()
            },
            collision_flags_player,

            collision_flags_weapon: value.prop("CollisionFlags_Weapon")?.try_into()?,
            collision_flags_interaction: value.prop("CollisionFlags_Interaction")?.try_into()?,
            collision_flags_physics: value.prop("CollisionFlags_Physics")?.try_into()?,
            visibility_flags: value.prop("VisibilityFlags")?.try_into()?,
            material_indices: value.prop("MaterialIndices")?.try_into()?,
            colors_and_alphas: {
                let mut colors: Vec<SavedBrickColor> =
                    value.prop("ColorsAndAlphas")?.try_into()?;
                // Older saves (and any missing the field) store linear colors;
                // default true and convert to sRGB so in-memory colors are
                // always sRGB. New saves store sRGB (bColorsAreLinear = false).
                let linear = if value.contains_key("bColorsAreLinear") {
                    value.prop("bColorsAreLinear")?.as_brdb_bool()?
                } else {
                    true
                };
                if linear {
                    for c in &mut colors {
                        *c = c.rgb_to_srgb();
                    }
                }
                colors
            },
            size_index_map: HashMap::new(),
            num_brick_sizes: 0,
        };
        for size_counter in &base.brick_size_counters {
            for j in 0..size_counter.num_sizes {
                let size_index =
                    (base.brick_sizes.len() - size_counter.num_sizes as usize + j as usize) as u32;
                base.size_index_map.insert(
                    (
                        size_counter.asset_index,
                        base.brick_sizes[size_index as usize],
                    ),
                    size_index,
                );
            }
            base.num_brick_sizes += size_counter.num_sizes;
        }
        Ok(base)
    }
}

#[derive(Default)]
pub struct BrickChunkIndexSoA {
    pub chunk_3d_indices: Vec<ChunkIndex>,
    pub chunk_offsets: Vec<IntVector>,
    pub chunk_sizes: Vec<i32>,
    pub num_bricks: Vec<u32>,
    pub num_components: Vec<u32>,
    pub num_wires: Vec<u32>,
}

impl AsBrdbValue for BrickChunkIndexSoA {
    fn as_brdb_struct_prop_array(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> Result<BrdbArrayIter<'_>, crate::errors::BrdbSchemaError> {
        match prop_name.get(schema).unwrap() {
            "Chunk3DIndices" => Ok(self.chunk_3d_indices.as_brdb_iter()),
            "ChunkOffsets" => Ok(self.chunk_offsets.as_brdb_iter()),
            "ChunkSizes" => Ok(self.chunk_sizes.as_brdb_iter()),
            "NumBricks" => Ok(self.num_bricks.as_brdb_iter()),
            "NumComponents" => Ok(self.num_components.as_brdb_iter()),
            "NumWires" => Ok(self.num_wires.as_brdb_iter()),
            n => unimplemented!("unimplemented struct field {n}"),
        }
    }
}
