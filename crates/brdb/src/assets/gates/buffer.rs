//! Buffer gates (`BrickComponentType_BufferTicks` / `_BufferSeconds`).
use crate::{
    BString, BrdbComponent, BrickType, WirePort,
    schema::{WireValue, as_brdb::AsBrdbValue},
};

macro_rules! buffer_gate {
    (
        $(#[$doc:meta])* $name:ident,
        component: $component:literal,
        struct_name: $struct_name:literal,
        brick: $brick:literal,
        wait: $wait:ident: $wait_ty:ty = $wait_field:literal,
        zero_wait: $zero:ident = $zero_field:literal,
        current: $current:ident = $current_field:literal $(,)?
    ) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Default)]
        pub struct $name {
            pub $wait: $wait_ty,
            pub $zero: $wait_ty,
            pub $current: $wait_ty,
            pub input: WireValue,
            pub output: WireValue,
            pub buffered: WireValue,
            pub has_queued: bool,
            pub is_off_timer: bool,
        }

        impl $name {
            pub const INPUT: BString = BString::str("Input");
            pub const OUTPUT: BString = BString::str("Output");
            pub const COMPONENT: BString = BString::str($component);
            pub const STRUCT_NAME: BString = BString::str($struct_name);
            pub const fn input_of(brick_id: usize) -> WirePort {
                WirePort {
                    brick_id,
                    component_type: Self::COMPONENT,
                    port_name: Self::INPUT,
                }
            }
            pub const fn output_of(brick_id: usize) -> WirePort {
                WirePort {
                    brick_id,
                    component_type: Self::COMPONENT,
                    port_name: Self::OUTPUT,
                }
            }
            pub const fn brick(&self) -> BrickType {
                BrickType::str($brick)
            }
            pub fn new(input: impl Into<WireValue>, output: impl Into<WireValue>) -> Self {
                Self {
                    input: input.into(),
                    output: output.into(),
                    ..Default::default()
                }
            }
        }

        impl AsBrdbValue for $name {
            fn as_brdb_struct_prop_value(
                &self,
                schema: &crate::schema::BrdbSchema,
                struct_name: crate::schema::BrdbInterned,
                prop_name: crate::schema::BrdbInterned,
            ) -> Result<&dyn AsBrdbValue, crate::errors::BrdbSchemaError> {
                match prop_name.get(schema).unwrap() {
                    $wait_field => Ok(&self.$wait),
                    $zero_field => Ok(&self.$zero),
                    $current_field => Ok(&self.$current),
                    "Input" => Ok(&self.input),
                    "Output" => Ok(&self.output),
                    "Buffered" => Ok(&self.buffered),
                    "bHasQueued" => Ok(&self.has_queued),
                    "bIsOffTimer" => Ok(&self.is_off_timer),
                    prop_name => Err(crate::errors::BrdbSchemaError::MissingStructField(
                        struct_name.get_or_else(schema, || "Unknown struct".to_owned()),
                        prop_name.to_owned(),
                    )),
                }
            }
        }

        impl BrdbComponent for $name {
            fn component_type(&self) -> Option<BString> {
                Some(Self::COMPONENT)
            }
        }
    };
}

buffer_gate!(
    /// Delays its input by a number of ticks.
    BufferTicks,
    component: "BrickComponentType_BufferTicks",
    struct_name: "BrickComponentData_BufferTicks",
    brick: "B_1x1_Gate_BufferTicks",
    wait: ticks_to_wait: i32 = "TicksToWait",
    zero_wait: zero_ticks_to_wait = "ZeroTicksToWait",
    current: current_ticks = "CurrentTicks",
);

buffer_gate!(
    /// Delays its input by a number of seconds.
    BufferSeconds,
    component: "BrickComponentType_BufferSeconds",
    struct_name: "BrickComponentData_BufferSeconds",
    brick: "B_1x1_Gate_BufferSeconds",
    wait: seconds_to_wait: f32 = "SecondsToWait",
    zero_wait: zero_seconds_to_wait = "ZeroSecondsToWait",
    current: current_time = "CurrentTime",
);
