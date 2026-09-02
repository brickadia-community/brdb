use std::{collections::HashMap, fmt::Display, sync::Arc};

use crate::{
    errors::BrdbSchemaError,
    schema::as_brdb::AsBrdbValue,
    wrapper::{BString, BrdbComponent},
};

#[derive(Clone)]
pub struct LiteralComponent {
    pub component_name: BString,
    pub data: Arc<HashMap<BString, Box<dyn AsBrdbValue>>>,
}

impl LiteralComponent {
    pub fn new(component_name: impl Into<BString>) -> Self {
        Self {
            component_name: component_name.into(),
            data: Default::default(),
        }
    }

    pub fn with_data(
        mut self,
        data: impl IntoIterator<Item = (impl Into<BString>, Box<dyn AsBrdbValue>)>,
    ) -> Self {
        self.data = Arc::new(data.into_iter().map(|(k, v)| (k.into(), v)).collect());
        self
    }

    pub fn new_from_data(
        component_name: impl Into<BString>,
        data: Arc<HashMap<BString, Box<dyn AsBrdbValue>>>,
    ) -> Self {
        Self {
            component_name: component_name.into(),
            data,
        }
    }
}

impl LiteralComponent {
    /// Key lookup tuned for the tiny maps literal components carry: a short
    /// linear scan beats hashing below a handful of entries.
    fn find(&self, name: &str) -> Option<&dyn AsBrdbValue> {
        if self.data.len() <= 4 {
            self.data
                .iter()
                .find(|(k, _)| k.as_ref() == name)
                .map(|(_, v)| v.as_ref())
        } else {
            self.data.get(name).map(|v| v.as_ref())
        }
    }
}

impl AsBrdbValue for LiteralComponent {
    /// A `LiteralComponent` written into a `variant`-typed field selects the
    /// variant member whose struct name matches `component_name` (its data is
    /// then written as that struct). Only consulted for variant fields; struct
    /// fields go through the struct accessors below.
    fn as_brdb_variant_member(&self) -> Option<&str> {
        Some(self.component_name.as_ref())
    }

    fn has_brdb_struct_props(&self) -> bool {
        !self.data.is_empty()
    }

    fn has_brdb_struct_prop(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> bool {
        prop_name
            .get(schema)
            .is_some_and(|name| self.find(name).is_some())
    }

    /// Single-probe fetch: one map lookup answers both presence and value.
    fn as_brdb_struct_prop_opt(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> Result<Option<&dyn AsBrdbValue>, crate::errors::BrdbSchemaError> {
        let Some(name) = prop_name.get(schema) else {
            return Ok(None);
        };
        Ok(self.find(name))
    }

    fn as_brdb_struct_prop_value(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, crate::errors::BrdbSchemaError> {
        let prop_name_str = prop_name.get(schema).unwrap();
        match self.find(prop_name_str) {
            Some(value) => Ok(value),
            None => Err(BrdbSchemaError::MissingStructField(
                self.component_name.to_string(),
                prop_name_str.to_string(),
            )),
        }
    }

    fn as_brdb_struct_prop_array(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> Result<crate::schema::as_brdb::BrdbArrayIter<'_>, crate::errors::BrdbSchemaError> {
        let prop_name_str = prop_name.get(schema).unwrap();
        match self.find(prop_name_str) {
            // Literal gate data only carries scalars; a stored value for an
            // array-typed field can't be iterated, so keep the loud default.
            Some(_) => Err(BrdbSchemaError::UnimplementedCast(
                "struct property array",
                std::any::type_name::<Self>(),
            )),
            // An unset array field (e.g. PrefabSpawner's SpawnedEntityIds) is
            // "missing" — the schema writer serializes it as an empty array,
            // matching the scalar missing-field behavior above.
            None => Err(BrdbSchemaError::MissingStructField(
                self.component_name.to_string(),
                prop_name_str.to_string(),
            )),
        }
    }
}

impl BrdbComponent for LiteralComponent {
    fn component_type(&self) -> Option<BString> {
        Some(self.component_name.clone())
    }

    fn component_type_ref(&self) -> Option<&str> {
        Some(self.component_name.as_ref())
    }
}

/// A literal component representing a seat
pub fn seat_component(
    allow_nearby: bool,
    hidden_interaction: bool,
    prompt_label: impl Display,
) -> LiteralComponent {
    LiteralComponent::new("Component_Internal_Seat").with_data([
        ("PlayerInput", Box::new(()) as Box<dyn AsBrdbValue>),
        ("bIsOccupied", Box::new(false)),
        ("bAllowNearbyInteraction", Box::new(allow_nearby)),
        ("bHiddenInteraction", Box::new(hidden_interaction)),
        ("PromptCustomLabel", Box::new(prompt_label.to_string())),
    ])
}
