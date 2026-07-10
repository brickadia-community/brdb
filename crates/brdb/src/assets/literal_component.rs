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

impl AsBrdbValue for LiteralComponent {
    fn has_brdb_struct_prop(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> bool {
        prop_name
            .get(schema)
            .is_some_and(|name| self.data.contains_key(name))
    }

    fn as_brdb_struct_prop_value(
        &self,
        schema: &crate::schema::BrdbSchema,
        _struct_name: crate::schema::BrdbInterned,
        prop_name: crate::schema::BrdbInterned,
    ) -> Result<&dyn AsBrdbValue, crate::errors::BrdbSchemaError> {
        let prop_name_str = prop_name.get(schema).unwrap();
        match self.data.get(prop_name_str) {
            Some(value) => Ok(value.as_ref()),
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
        match self.data.get(prop_name_str) {
            // Literal gate data only carries scalars; a stored value for an
            // array-typed field can't be iterated, so keep the loud default.
            Some(_) => Err(BrdbSchemaError::UnimplementedCast(
                "struct property array".to_owned(),
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
