use crate::geometry::PlantGeoData;
use crate::shape::pdms_shape::PlantMesh;
use crate::{types::*, GeomInstQuery, SUL_DB};
use approx::{abs_diff_ne, assert_abs_diff_eq, AbsDiffEq};
use bevy_ecs::prelude::Resource;
use dashmap::mapref::one::Ref;
use dashmap::DashMap;
use glam::{Mat4, Vec3};
use parry3d::bounding_volume::Aabb;
use parry3d::query::{Ray, RayCast};
use parry3d::shape::TriMesh;
use parry3d::shape::TriMeshFlags;
use rstar::Envelope;
use serde_derive::{Deserialize, Serialize};
use serde_with::{serde_as, As, FromInto};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Write};
use std::ops::{Deref, DerefMut};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RStarBoundingBox {
    pub aabb: Aabb,
    #[serde(serialize_with = "RefU64::serialize_as_u64")]
    #[serde(deserialize_with = "RefU64::deserialize_from_u64")]
    pub refno: RefU64,
    //方便过滤
    pub noun: String,
}

impl RStarBoundingBox {
    pub fn new(aabb: Aabb, refno: RefnoEnum, noun: String) -> Self {
        Self {
            aabb,
            refno: refno.refno(),
            noun,
        }
    }

    pub fn from_aabb(aabb: Aabb, refno: RefnoEnum) -> Self {
        Self {
            aabb,
            refno: refno.refno(),
            noun: "UNSET".to_string(),
        }
    }

    pub fn from_min_max(min: Vec3, max: Vec3, transform: Mat4, refno: RefnoEnum) -> Self {
        let min = transform.transform_point3(min);
        let max = transform.transform_point3(max);

        Self {
            aabb: Aabb::new(min.into(), max.into()),
            refno: refno.refno(),
            noun: "UNSET".to_string(),
        }
    }
}

impl rstar::RTreeObject for RStarBoundingBox {
    type Envelope = rstar::AABB<[f32; 3]>;

    fn envelope(&self) -> Self::Envelope {
        rstar::AABB::from_corners(self.aabb.mins.into(), self.aabb.maxs.into())
    }
}

impl rstar::PointDistance for RStarBoundingBox {
    fn distance_2(&self, point: &[f32; 3]) -> f32 {
        let aabb = rstar::AABB::from_corners(self.aabb.mins.into(), self.aabb.maxs.into());
        aabb.distance_2(point)
    }
}

#[serde_as]
#[derive(Clone, Default, Serialize, Deserialize, Resource)]
pub struct AccelerationTree {
    pub tree: rstar::RTree<RStarBoundingBox>,
    //用来检查是否插入到了 Tree，如果遇到重复的，需要跳过
    #[serde_as(as = "HashSet<FromInto<u64>>")]
    ids: HashSet<RefU64>,
    #[serde(skip)]
    mesh_cache: DashMap<RefnoEnum, Vec<TriMesh>>,
}

impl Deref for AccelerationTree {
    type Target = rstar::RTree<RStarBoundingBox>;

    fn deref(&self) -> &Self::Target {
        &self.tree
    }
}

impl DerefMut for AccelerationTree {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.tree
    }
}

pub struct QueryRay {
    pub ray: Ray,
    pub filter_nouns: HashSet<String>,
    //需要使用当前的房间的距离，这样可以缩小范围
    pub toi: f32,
    pub solid: bool,
    pub min_dist: Cell<f32>,
    pub min_refnos: HashSet<RefnoEnum>,
}

impl QueryRay {
    pub fn new(ray: Ray, filter_nouns: HashSet<String>, solid: bool) -> Self {
        Self {
            ray,
            filter_nouns,
            toi: 10_0000.0,
            // found: Cell::new(false),
            min_dist: Cell::new(f32::MAX),
            min_refnos: HashSet::default(),
            solid,
        }
    }
}

impl rstar::SelectionFunction<RStarBoundingBox> for &QueryRay {
    //如果找到了最近的，应该停止继续搜索
    fn should_unpack_parent(&self, envelope: &rstar::AABB<[f32; 3]>) -> bool {
        use parry3d::{math::*, query::*};
        //如果已经找到了，就可以返回 false
        // if self.found.get() {
        //     return false;
        // }
        let bbox = Aabb::new(envelope.lower().into(), envelope.upper().into());
        // dbg!(&bbox);
        bbox.intersects_ray(&Isometry::identity(), &self.ray, self.toi)
    }

    fn should_unpack_leaf(&self, bbox: &RStarBoundingBox) -> bool {
        use parry3d::{math::*, query::*};
        if !self.filter_nouns.is_empty() && self.filter_nouns.contains(&bbox.noun) {
            //每次查找的距离应该比这个小，否则跳过
            let inter = bbox.aabb.cast_ray_and_get_normal(
                &Isometry::identity(),
                &self.ray,
                self.toi,
                self.solid,
            );
            // dbg!(&inter);
            if let Some(ray_inter) = inter
                && ray_inter.time_of_impact <= self.min_dist.get()
            {
                self.min_dist.set(ray_inter.time_of_impact);

                //找到更近的，清空之前的
                // if abs_diff_ne!(ray_inter.time_of_impact, self.toi) {
                //     self.min_refnos.borrow_mut().clear()
                // }
                // self.min_refnos.borrow_mut().insert(bbox.refno.into());
                // println!("found: {}", bbox.refno);
                // dbg!(ray_inter.toi);
                return true;
            }
        }
        return false;
    }
}

impl AccelerationTree {
    #[inline]
    pub fn size(&self) -> usize {
        self.tree.size()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.tree.size() == 0
    }

    /// 加载包围盒
    pub fn load(mut bounding_boxes: Vec<RStarBoundingBox>) -> Self {
        Self {
            tree: rstar::RTree::bulk_load(bounding_boxes),
            ..Default::default()
        }
    }

    /// 新加数据
    pub fn update_aabbs(&mut self, bboxes: Vec<RStarBoundingBox>) {
        //检查 refno 是否已经存在了，如果存在，先移除，再添加进去
        for bbox in bboxes {
            if self.ids.insert(bbox.refno) {
                self.tree.remove(&bbox);
            }
            self.tree.insert(bbox);
        }
    }

    pub fn replace(&mut self, bounding_boxes: Vec<RStarBoundingBox>) {
        self.tree = rstar::RTree::bulk_load(bounding_boxes);
    }

    pub fn query_within_distance<'a>(
        &'a self,
        loc: Vec3,
        distance: f32,
    ) -> impl Iterator<Item = (RefnoEnum, Aabb)> + 'a {
        self.tree
            .locate_within_distance([loc.x, loc.y, loc.z], distance.powi(2))
            .map(|bb| (bb.refno.into(), bb.aabb))
    }

    pub fn locate_intersecting_bounds<'a>(
        &'a self,
        bounds: &Aabb,
    ) -> impl Iterator<Item = &RStarBoundingBox> + 'a {
        self.tree
            .locate_in_envelope_intersecting(&rstar::AABB::from_corners(
                [bounds.mins[0], bounds.mins[1], bounds.mins[2]],
                [bounds.maxs[0], bounds.maxs[1], bounds.maxs[2]],
            ))
            .map(|bb| bb)
    }

    /// 检查是否包含包围盒
    pub fn locate_contain_bounds<'a>(
        &'a self,
        bounds: &Aabb,
    ) -> impl Iterator<Item = &RStarBoundingBox> + 'a {
        self.tree
            .locate_in_envelope(&rstar::AABB::from_corners(
                [bounds.mins[0], bounds.mins[1], bounds.mins[2]],
                [bounds.maxs[0], bounds.maxs[1], bounds.maxs[2]],
            ))
            .map(|bb| bb)
    }

    //实现使用bincode序列化
    #[cfg(not(target_arch = "wasm32"))]
    pub fn serialize_to_bin_file(&self) -> anyhow::Result<bool> {
        let mut file = File::create("accel_tree.bin")?;
        let serialized = bincode::serialize(&self)?;
        file.write_all(serialized.as_slice())?;

        Ok(true)
    }

    /// 使用bincode反序列化
    #[cfg(not(target_arch = "wasm32"))]
    pub fn deserialize_from_bin_file() -> anyhow::Result<Self> {
        let mut file = File::open("accel_tree.bin")?;
        let mut buf: Vec<u8> = Vec::new();
        let _ = file.read_to_end(&mut buf)?;
        let r = bincode::deserialize(&buf).unwrap();
        Ok(r)
    }

    /// 获取一个refno的mesh
    /// 如果mesh_cache中没有，则从数据库中加载
    /// 如果数据库中也没有，则返回None
    pub async fn get_tri_mesh(&self, refno: RefnoEnum) -> Option<Ref<RefnoEnum, Vec<TriMesh>>> {
        if let Some(r) = self.mesh_cache.get(&refno) {
            return Some(r);
        }
        let geom_insts = crate::query_insts(&[refno], true).await.ok()?;
        // dbg!(geom_insts.len());
        let mut meshes = vec![];
        for g in geom_insts {
            // dbg!(&g);
            for inst in &g.insts {
                let Ok(mesh) =
                    PlantMesh::des_mesh_file(&format!("assets/meshes/{}.mesh", inst.geo_hash))
                else {
                    continue;
                };
                // dbg!(mesh.vertices.len());
                if mesh.vertices.is_empty() {
                    continue;
                }
                let trans = g.world_trans * inst.transform;
                let Some(tri_mesh) = mesh.get_tri_mesh(trans.compute_matrix()) else {
                    continue;
                };
                meshes.push(tri_mesh);
            }
        }
        self.mesh_cache.insert(refno, meshes);
        return self.mesh_cache.get(&refno);
    }

    /// Returns the refno of the nearest object to `query_point`
    /// 也可以检查墙等等，需要判断mesh
    pub async fn query_nearest_by_ray(
        &self,
        ray: QueryRay,
    ) -> anyhow::Result<Option<(RefnoEnum, f32)>> {
        let _ = self
            .tree
            .locate_with_selection_function(&ray)
            .collect::<Vec<_>>();
        let refnos = &ray.min_refnos;
        if refnos.is_empty() {
            return Ok(None);
        }
        //检查是否真的和ray相交, 根据 profile 判断吗？
        //根据 param 判断是否相交吗？
        //直接检查是否在表面上即可
        for &refno in refnos.iter() {
            let Some(tri_meshes) = self.get_tri_mesh(refno).await else {
                continue;
            };
            for mesh in tri_meshes.value() {
                let intersection_flag =
                    match mesh.cast_local_ray_and_get_normal(&ray.ray, 10_0000.0, ray.solid) {
                        // Some(intersection) => tri_mesh.is_backface(intersection.feature),
                        Some(intersection) => {
                            // dbg!(&intersection);
                            true
                        }
                        None => false,
                    };
                if intersection_flag {
                    return Ok(Some((refno, ray.min_dist.get())));
                }
            }
        }
        return Ok(None);
    }
}
