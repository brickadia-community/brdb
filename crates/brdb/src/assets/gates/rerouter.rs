use crate::{
    schema::as_brdb::AsBrdbValue,
    wrapper::{BString, BrdbComponent, WirePort},
};

#[derive(Debug, Clone, Copy)]
pub struct Rerouter;
impl AsBrdbValue for Rerouter {}
impl Rerouter {
    pub const fn brick(&self) -> crate::BrickType {
        super::super::bricks::B_REROUTE
    }
    pub const INPUT: BString = BString::str("RER_Input");
    pub const OUTPUT: BString = BString::str("RER_Output");
    pub const COMPONENT: BString = BString::str("Component_Internal_Rerouter");
    pub const fn input_of(brick_id: usize) -> WirePort {
        WirePort {
            brick_id,
            component_type: Rerouter::COMPONENT,
            port_name: Rerouter::INPUT,
        }
    }
    pub const fn output_of(brick_id: usize) -> WirePort {
        WirePort {
            brick_id,
            component_type: Rerouter::COMPONENT,
            port_name: Rerouter::OUTPUT,
        }
    }
}
impl BrdbComponent for Rerouter {
    fn component_type(&self) -> Option<BString> {
        Some(Self::COMPONENT)
    }
}
