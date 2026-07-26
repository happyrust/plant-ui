use std::vec::IntoIter;
use serde_with::serde_as;
use serde_derive::{Deserialize, Serialize};
use bevy_ecs::component::Component;
use derive_more::{Deref, DerefMut};
use crate::cache::mgr::BytesTrait;
use crate::{query_refno_sesno, RefU64};

#[serde_as]
#[derive(
    Serialize,
    Deserialize,
    Clone,
    Debug,
    Default,
    Component,
    Deref,
    DerefMut,
    rkyv::Archive,
    rkyv::Deserialize,
    rkyv::Serialize,
)]
pub struct RefU64Vec(pub Vec<RefU64>);


impl RefU64Vec {
   
}

impl BytesTrait for RefU64Vec {}

impl From<Vec<RefU64>> for RefU64Vec {
    fn from(d: Vec<RefU64>) -> Self {
        RefU64Vec(d)
    }
}

impl IntoIterator for RefU64Vec {
    type Item = RefU64;
    type IntoIter = IntoIter<RefU64>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl RefU64Vec {
    #[inline]
    pub fn push(&mut self, v: RefU64) {
        if !self.0.contains(&v) {
            self.0.push(v);
        }
    }
}
