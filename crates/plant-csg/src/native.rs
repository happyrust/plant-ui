//! 原生端后端：专用线程跑 `manifold-csg`，mpsc 收发作业；按 `mesh_key` 缓存焊接
//! 实体（LRU 64）。UI 帧只负责 submit / poll，布尔运算不占帧线程。

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::Instant;

use glam::DMat4;
use manifold_csg::Manifold;

use crate::convert::{manifold_to_mesh, mesh_to_manifold};
use crate::{
    BooleanJob, BooleanOutcome, BooleanPreviewBackend, BooleanStatus, CutterShape,
};

/// 焊接实体缓存上限。
const CACHE_CAP: usize = 64;

/// 体积相对变化小于这个值且三角数不变，判「没有实质相交」。
const NO_INTERSECTION_VOLUME_EPS: f64 = 1e-9;

enum Msg {
    Job(BooleanJob),
    Forget(u64),
}

/// 简单 LRU：`HashMap` 存实体，`VecDeque` 记访问序。
struct Lru {
    map: HashMap<u64, Manifold>,
    order: VecDeque<u64>,
}

impl Lru {
    fn get(&mut self, key: u64) -> Option<Manifold> {
        let solid = self.map.get(&key)?.clone();
        self.order.retain(|&k| k != key);
        self.order.push_back(key);
        Some(solid)
    }
    fn insert(&mut self, key: u64, solid: Manifold) {
        if self.map.contains_key(&key) {
            self.order.retain(|&k| k != key);
        } else if self.map.len() >= CACHE_CAP {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            }
        }
        self.map.insert(key, solid);
        self.order.push_back(key);
    }
    fn remove(&mut self, key: u64) {
        self.map.remove(&key);
        self.order.retain(|&k| k != key);
    }
}

impl Default for Lru {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }
}

/// 跑一个作业。失败一律保留原网格（ADR-029：空结果不覆盖）。
fn run_job(cache: &mut Lru, job: BooleanJob) -> BooleanOutcome {
    let started = Instant::now();
    let finish = |status, mesh, volume_before, volume_after, tri_count, error: Option<String>| {
        BooleanOutcome {
            job_id: job.job_id,
            epoch: job.epoch,
            status,
            mesh,
            volume_before,
            volume_after,
            tri_count,
            elapsed_ms: started.elapsed().as_millis() as u64,
            error,
        }
    };
    let target = match cache.get(job.mesh_key) {
        Some(solid) => solid,
        None => {
            let Some(mesh) = job.mesh.clone() else {
                return finish(BooleanStatus::NeedMesh, None, 0.0, 0.0, 0, None);
            };
            match mesh_to_manifold(&mesh.positions, &mesh.indices, DMat4::IDENTITY) {
                Ok(solid) => {
                    cache.insert(job.mesh_key, solid.clone());
                    solid
                }
                Err(error) => {
                    return finish(BooleanStatus::Failed, None, 0.0, 0.0, 0, Some(error));
                }
            }
        }
    };
    let volume_before = target.volume();
    let tri_before = target.num_tri();
    // 同一目标的全部切削体合并成一次 difference，结果与孔洞顺序无关。
    // 膨胀（每边让 0.051mm）在上游 cutter_affine 里已经折进尺寸。
    let mut group = Vec::with_capacity(1 + job.cutters.len());
    group.push(target);
    for cutter in &job.cutters {
        let prim = match cutter.shape {
            CutterShape::Circle => Manifold::cylinder(
                1.0,
                0.5,
                0.5,
                cutter.segments.max(3) as i32,
                true,
            ),
            CutterShape::Rect => Manifold::cube(1.0, 1.0, 1.0, true),
        };
        group.push(prim.transform(&cutter.affine));
    }
    let result = Manifold::batch_difference(&group);
    if let Err(error) = result.status() {
        return finish(
            BooleanStatus::Failed,
            None,
            volume_before,
            0.0,
            0,
            Some(format!("manifold status: {error}")),
        );
    }
    if result.is_empty() {
        return finish(
            BooleanStatus::Failed,
            None,
            volume_before,
            0.0,
            0,
            Some("difference result is empty".into()),
        );
    }
    let volume_after = result.volume();
    let tri_count = result.num_tri();
    let unchanged = (volume_after - volume_before).abs()
        <= NO_INTERSECTION_VOLUME_EPS * volume_before.abs().max(1.0)
        && tri_count == tri_before;
    if unchanged {
        return finish(
            BooleanStatus::NoIntersection,
            None,
            volume_before,
            volume_after,
            tri_count,
            None,
        );
    }
    match manifold_to_mesh(&result) {
        Some(mesh) => finish(
            BooleanStatus::Changed,
            Some(mesh),
            volume_before,
            volume_after,
            tri_count,
            None,
        ),
        None => finish(
            BooleanStatus::Failed,
            None,
            volume_before,
            volume_after,
            tri_count,
            Some("result mesh has no triangles".into()),
        ),
    }
}

/// 原生端后端。`Receiver` 本身不是 Sync，套一层 `Mutex` 才能住进 Bevy 资源
///（每帧 poll 只短暂持锁，布尔运算在工作线程上，不受影响）。
pub struct NativeBackend {
    tx: Sender<Msg>,
    rx: Mutex<Receiver<BooleanOutcome>>,
}

impl NativeBackend {
    pub fn new() -> Self {
        let (job_tx, job_rx) = channel::<Msg>();
        let (out_tx, out_rx) = channel::<BooleanOutcome>();
        std::thread::Builder::new()
            .name("plant-csg".into())
            .spawn(move || {
                let mut cache = Lru::default();
                while let Ok(msg) = job_rx.recv() {
                    match msg {
                        Msg::Job(job) => {
                            let _ = out_tx.send(run_job(&mut cache, job));
                        }
                        Msg::Forget(key) => cache.remove(key),
                    }
                }
            })
            .expect("spawn plant-csg worker thread");
        Self {
            tx: job_tx,
            rx: Mutex::new(out_rx),
        }
    }
}

impl Default for NativeBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl BooleanPreviewBackend for NativeBackend {
    fn submit(&mut self, job: BooleanJob) {
        let _ = self.tx.send(Msg::Job(job));
    }
    fn poll(&mut self) -> Vec<BooleanOutcome> {
        let mut out = Vec::new();
        let rx = self.rx.lock().expect("plant-csg recv lock poisoned");
        while let Ok(outcome) = rx.try_recv() {
            out.push(outcome);
        }
        out
    }
    fn forget(&mut self, mesh_key: u64) {
        let _ = self.tx.send(Msg::Forget(mesh_key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MeshData;
    use std::sync::Arc;

    fn cube_mesh(size: f32, at: [f32; 3]) -> MeshData {
        let p: Vec<[f32; 3]> = (0..8)
            .map(|i| {
                [
                    at[0] + if i & 1 == 0 { 0.0 } else { size },
                    at[1] + if i & 2 == 0 { 0.0 } else { size },
                    at[2] + if i & 4 == 0 { 0.0 } else { size },
                ]
            })
            .collect();
        let faces = [
            [0, 2, 3, 0, 3, 1],
            [4, 5, 7, 4, 7, 6],
            [0, 1, 5, 0, 5, 4],
            [2, 6, 7, 2, 7, 3],
            [1, 3, 7, 1, 7, 5],
            [0, 4, 6, 0, 6, 2],
        ];
        let mut positions = Vec::new();
        let mut indices = Vec::new();
        for f in faces {
            let base = positions.len() as u32;
            for &v in &f {
                positions.push(p[v]);
            }
            indices.extend([base, base + 1, base + 2, base + 3, base + 4, base + 5]);
        }
        MeshData { positions, indices }
    }

    /// 单位圆柱基元放到 `affine`（列主序 4×3）的位置：箱 100×100×100 在原点，
    /// 切削体是沿 Z 的 Φ40、长 300 的圆柱，中心 (50, 50, 50)。
    fn through_cylinder_job(mesh: Arc<MeshData>, mesh_key: u64) -> BooleanJob {
        let size = [40.102, 40.102, 300.102]; // 膨胀后的尺寸（诊断字段）
        let affine = [
            size[0], 0.0, 0.0, // x 轴
            0.0, size[1], 0.0, // y 轴
            0.0, 0.0, size[2], // z 轴
            50.0, 50.0, 50.0, // 平移（末列）
        ];
        BooleanJob {
            job_id: 1,
            epoch: 1,
            mesh_key,
            mesh: Some(mesh),
            cutters: vec![crate::CutterJob {
                shape: CutterShape::Circle,
                size_mm: size,
                affine,
                segments: 64,
            }],
        }
    }

    fn run(job: BooleanJob) -> BooleanOutcome {
        let mut backend = NativeBackend::new();
        backend.submit(job);
        let deadline = Instant::now() + std::time::Duration::from_secs(30);
        loop {
            // 一次 poll 可能带回多条结果，但这里一次只提交了一个作业，取第一条就够；
            // 用 `if let` 而不是 `for ... return`：后者循环体第一拍就返回，clippy 判成
            // `never_loop`（deny 级）。
            if let Some(outcome) = backend.poll().into_iter().next() {
                return outcome;
            }
            assert!(Instant::now() < deadline, "boolean timed out");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn through_hole_removes_cylinder_volume() {
        let mesh = Arc::new(cube_mesh(100.0, [0.0, 0.0, 0.0]));
        let key = crate::mesh_key(&mesh);
        let outcome = run(through_cylinder_job(mesh, key));
        assert_eq!(outcome.status, BooleanStatus::Changed, "{:?}", outcome.error);
        let mesh = outcome.mesh.unwrap();
        // 体积减少 ≈ π r² h（r=20, h=100 穿透）。
        let removed = outcome.volume_before - outcome.volume_after;
        let expect = std::f64::consts::PI * 20.0 * 20.0 * 100.0;
        assert!(
            (removed - expect).abs() / expect < 0.01,
            "removed={removed} expect={expect}"
        );
        assert!(mesh.tri_count() > 12);
    }

    #[test]
    fn far_away_cutter_is_no_intersection() {
        let mesh = Arc::new(cube_mesh(100.0, [0.0, 0.0, 0.0]));
        let key = crate::mesh_key(&mesh);
        let mut job = through_cylinder_job(mesh, key);
        // 切削体挪到箱外面。
        job.cutters[0].affine[9] = 10_000.0;
        let outcome = run(job);
        assert_eq!(outcome.status, BooleanStatus::NoIntersection);
        assert!(outcome.mesh.is_none());
    }

    #[test]
    fn two_cutters_match_any_order() {
        let mesh = Arc::new(cube_mesh(200.0, [0.0, 0.0, 0.0]));
        let key = crate::mesh_key(&mesh);
        let mut cutters = Vec::new();
        for cx in [50.0, 150.0] {
            cutters.push(crate::CutterJob {
                shape: CutterShape::Circle,
                size_mm: [40.102, 40.102, 300.102],
                affine: [40.102, 0.0, 0.0, 0.0, 40.102, 0.0, 0.0, 0.0, 300.102, cx, 100.0, 100.0],
                segments: 64,
            });
        }
        let mut job = BooleanJob {
            job_id: 1,
            epoch: 1,
            mesh_key: key,
            mesh: Some(mesh),
            cutters: cutters.clone(),
        };
        let a = run(job.clone());
        cutters.swap(0, 1);
        job.cutters = cutters;
        let b = run(job);
        assert_eq!(a.status, BooleanStatus::Changed);
        assert_eq!(b.status, BooleanStatus::Changed);
        assert_eq!(a.tri_count, b.tri_count, "cutter order must not matter");
        assert!((a.volume_after - b.volume_after).abs() < 1e-6);
    }

    #[test]
    fn open_mesh_fails_and_keeps_original() {
        let mut mesh = cube_mesh(100.0, [0.0, 0.0, 0.0]);
        mesh.indices.truncate(mesh.indices.len() - 6);
        let key = crate::mesh_key(&mesh);
        let outcome = run(through_cylinder_job(Arc::new(mesh), key));
        assert_eq!(outcome.status, BooleanStatus::Failed);
        assert!(outcome.mesh.is_none());
        assert!(outcome.error.is_some());
    }

    #[test]
    fn rect_cutter_works() {
        let mesh = Arc::new(cube_mesh(100.0, [0.0, 0.0, 0.0]));
        let key = crate::mesh_key(&mesh);
        let mut job = through_cylinder_job(mesh, key);
        job.cutters[0].shape = CutterShape::Rect;
        job.cutters[0].size_mm = [30.102, 50.102, 300.102];
        job.cutters[0].affine = [
            30.102, 0.0, 0.0, 0.0, 50.102, 0.0, 0.0, 0.0, 300.102, 50.0, 50.0, 50.0,
        ];
        let outcome = run(job);
        assert_eq!(outcome.status, BooleanStatus::Changed, "{:?}", outcome.error);
        let removed = outcome.volume_before - outcome.volume_after;
        let expect = 30.0 * 50.0 * 100.0;
        assert!((removed - expect).abs() / expect < 0.01);
    }
}
