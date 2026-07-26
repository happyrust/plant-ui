pub mod helper;
pub mod sbox;
pub mod sphere;
pub mod cylinder;
pub mod snout;
pub mod dish;
pub mod ctorus;
pub mod extrusion;
pub mod revolution;
pub mod pyramid;
pub mod lpyramid;
pub mod rtorus;
pub mod facet;
pub mod sweep_solid;
pub mod tubing;
pub mod polyhedron;

pub mod category;
pub mod spine;

pub mod wire;

pub mod line;

pub mod basic;

pub mod profile;

use dashmap::DashMap;
pub use sbox::*;
pub use sphere::*;
pub use cylinder::*;
pub use snout::*;
pub use dish::*;
pub use ctorus::*;
pub use extrusion::*;
pub use revolution::*;
pub use pyramid::*;
pub use lpyramid::*;
pub use rtorus::*;
pub use facet::*;
pub use sweep_solid::*;
pub use tubing::*;
pub use polyhedron::*;
use crate::prim_geo::category::CateBrepShape;
use crate::{RefU64, RefnoEnum};

pub type CateBrepShapeMap = DashMap<RefnoEnum, Vec<CateBrepShape>>;


