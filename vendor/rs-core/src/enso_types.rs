use crate::types::named_attvalue::NamedAttrValue;
use bevy_ecs::prelude::Component;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Component)]
pub struct EnsoJson {
    pub headers: Vec<String>,
    pub values: Vec<Vec<NamedAttrValue>>,
}
