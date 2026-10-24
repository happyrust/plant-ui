//! 虚拟孔洞布尔预览的后端抽象（ADR-0031）。
//!
//! 本 crate 不依赖 Bevy。View3d 从实例实体上取下局部网格，把切削体换算到同一局部系，
//! 整个作业在毫米 f64 口径下提交给后端；结果网格直接换回实例的渲染网格，不回宿主。
//!
//! - 原生端：[`native`]（feature `manifold`），专用线程跑 `manifold-csg`，是 gen-model
//!   ADR-029 同一份转换逻辑的移植（见 [`convert`]）。
//! - 浏览器端：[`web`]，`web/public/virtual-hole/worker.js` 里跑官方 `manifold-3d`
//!   （npm 3.5.4，内核与原生端 C++ v3.5.2 一致），算法在同目录 `boolean.js`。
//!
//! 两端的焊接、退化三角形判据、段数口径必须逐行一致——改一处要改另一处，
//! 对拍靠 `scripts/virtual-hole-parity.mjs`。

use std::sync::Arc;

#[cfg(all(not(target_arch = "wasm32"), feature = "manifold"))]
pub mod convert;
#[cfg(all(not(target_arch = "wasm32"), feature = "manifold"))]
pub mod native;
#[cfg(target_arch = "wasm32")]
pub mod web;

/// 切削体基元形状。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutterShape {
    /// 圆柱（圆孔）。
    Circle,
    /// 长方体（矩形孔）。
    Rect,
}

/// 渲染网格：局部坐标（毫米），f32 顶点 + u32 索引。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl MeshData {
    /// 三角形数量。
    pub fn tri_count(&self) -> usize {
        self.indices.len() / 3
    }
}

/// 一个切削体：单位基元 + 局部系仿射（列主序 4×3，毫米）。
///
/// 基元是单位几何：圆柱底面半径 0.5、高 1，或 1×1×1 箱，都以原点为中心、轴沿 Z。
/// `affine` 把它放进目标局部系。负体膨胀（每边让 0.051mm，libgm `restol` 口径）
/// 在上游 `VirtualHole::cutter_affine` 里已经折进尺寸，这里不再处理。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutterJob {
    pub shape: CutterShape,
    /// 膨胀后的尺寸（诊断用；几何完全由 `affine` 表达）。
    pub size_mm: [f64; 3],
    /// 列主序 4×3 仿射：单位基元 → 目标局部系（毫米）。
    pub affine: [f64; 12],
    /// 圆柱离散段数（与渲染层显示共用 `plant_ui::virtual_hole::circle_segments` 口径）。
    pub segments: u32,
}

/// 一次布尔作业：一个目标实例减去它的全部切削体（合并为一次 difference，
/// 与孔洞顺序无关）。
#[derive(Debug, Clone)]
pub struct BooleanJob {
    pub job_id: u64,
    pub epoch: u64,
    /// 目标网格内容的稳定键（宿主按内容哈希算）；后端按它缓存焊接实体。
    pub mesh_key: u64,
    /// 目标网格（局部毫米）。调用方总是提供；后端在缓存命中时忽略它。
    pub mesh: Option<Arc<MeshData>>,
    /// 该实例上所有相交切削体（局部毫米坐标）。
    pub cutters: Vec<CutterJob>,
}

/// 作业结果状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanStatus {
    /// 布尔成功且体积 / 三角数有变化，`mesh` 是结果网格。
    Changed,
    /// 与切削体没有实质相交（体积相对变化 < 1e-9 且三角数不变）。
    NoIntersection,
    /// 失败（非流形输入、空结果、内核报错）；保留原网格。
    Failed,
    /// 后端缓存未命中且没带网格；调用方去掉缓存标记、带网格重提一次。
    NeedMesh,
}

/// 一次作业的结果。
#[derive(Debug, Clone)]
pub struct BooleanOutcome {
    pub job_id: u64,
    pub epoch: u64,
    pub status: BooleanStatus,
    /// `Changed` 时为结果网格（局部毫米）。
    pub mesh: Option<MeshData>,
    pub volume_before: f64,
    pub volume_after: f64,
    pub tri_count: usize,
    pub elapsed_ms: u64,
    pub error: Option<String>,
}

/// 后端统一接口。原计划书里的 `SolidHandle` 句柄跨不过线程与 Worker 边界，
/// 降为按作业提交 + 轮询取结果。要求 Send + Sync：后端要住进 Bevy 的 `Resource`
/// （浏览器端内部靠 wasm 单线程兜底，见 web.rs 的 SendSync 包装）。
pub trait BooleanPreviewBackend: Send + Sync {
    /// 提交一个作业（异步执行）。
    fn submit(&mut self, job: BooleanJob);
    /// 取走已完成的结果（每帧轮询）。
    fn poll(&mut self) -> Vec<BooleanOutcome>;
    /// 释放某个目标网格的缓存（模型卸载时调用）。
    fn forget(&mut self, mesh_key: u64);
}

/// 按平台与 feature 拣一个后端。原生端关掉 `virtual-hole-csg` 时落到兜底。
pub fn backend() -> Box<dyn BooleanPreviewBackend> {
    #[cfg(all(not(target_arch = "wasm32"), feature = "manifold"))]
    {
        return Box::new(native::NativeBackend::new());
    }
    #[cfg(all(not(target_arch = "wasm32"), not(feature = "manifold")))]
    {
        return Box::new(UnavailableBackend::default());
    }
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(web) = web::WebBackend::new() {
            return Box::new(web);
        }
        Box::new(UnavailableBackend::default())
    }
}

/// 未启用后端时的兜底：作业立刻拍成 `Failed`，界面马上能看到原因。
#[derive(Debug, Default)]
pub struct UnavailableBackend {
    pending: Vec<BooleanJob>,
}

impl BooleanPreviewBackend for UnavailableBackend {
    fn submit(&mut self, job: BooleanJob) {
        self.pending.push(job);
    }
    fn poll(&mut self) -> Vec<BooleanOutcome> {
        self.pending
            .drain(..)
            .map(|job| BooleanOutcome {
                job_id: job.job_id,
                epoch: job.epoch,
                status: BooleanStatus::Failed,
                mesh: None,
                volume_before: 0.0,
                volume_after: 0.0,
                tri_count: 0,
                elapsed_ms: 0,
                error: Some("此版本未启用布尔预览（virtual-hole-csg）".into()),
            })
            .collect()
    }
    fn forget(&mut self, _mesh_key: u64) {}
}

// ─── 两端共享的纯函数（浏览器端在 boolean.js 里逐行镜像） ───────────────────────

/// 焊接 / 退化判据的距离口径（mm）：0.1µm。与 gen-model `manifold_csg.rs` 的
/// `WELD_EPS` 同值；判据统一在 f64 上算（f32 输入在 f64 里精确），保证两端逐位一致。
pub const WELD_EPS: f32 = 1e-4;

/// 焊接键：f64 坐标位模式，`-0.0` 归一成 `0.0`（与 gen-model 同口径；JS 端
/// `String(-0) === "0"` 天然等价）。
pub fn weld_key(x: f64, y: f64, z: f64) -> [u64; 3] {
    let canon = |v: f64| if v == 0.0 { 0.0 } else { v };
    [canon(x).to_bits(), canon(y).to_bits(), canon(z).to_bits()]
}

/// 把三角网格按精确坐标焊接成共享顶点网格：输出 f64 顶点（展平，每点 3 个）与
/// u64 索引。PlantMesh 为硬边渲染会按面复制顶点；Manifold 的输入拓扑要求相邻面
/// 共享同一顶点编号，所以焊接是必经之路。f32 入参在 f64 里精确，直接委托。
pub fn weld_mesh(positions: &[[f32; 3]], indices: &[u32]) -> (Vec<f64>, Vec<u64>) {
    let wide: Vec<[f64; 3]> = positions
        .iter()
        .map(|p| [p[0] as f64, p[1] as f64, p[2] as f64])
        .collect();
    weld_mesh_f64(&wide, indices)
}

/// [`weld_mesh`] 的 f64 入参版：先变换再焊接时用（gen-model 口径——焊接键落在
/// 变换后的 f64 坐标上）。索引越界的位置得 `u64::MAX` 哨兵，调用方负责检查。
pub fn weld_mesh_f64(positions: &[[f64; 3]], indices: &[u32]) -> (Vec<f64>, Vec<u64>) {
    let mut vert_props = Vec::with_capacity(positions.len() * 3);
    let mut vertex_ids = Vec::with_capacity(positions.len());
    let mut welded = std::collections::HashMap::<[u64; 3], u64>::new();
    for v in positions {
        let next = welded.len() as u64;
        let id = *welded
            .entry(weld_key(v[0], v[1], v[2]))
            .or_insert_with(|| {
                vert_props.push(v[0]);
                vert_props.push(v[1]);
                vert_props.push(v[2]);
                next
            });
        vertex_ids.push(id);
    }
    let tri = indices
        .iter()
        .map(|&index| vertex_ids.get(index as usize).copied().unwrap_or(u64::MAX))
        .collect();
    (vert_props, tri)
}

/// 退化三角形判据（mm）：任一边长不超过 `WELD_EPS`（f64 上算），或 f64 零面积。
/// 塌掉的三角丢掉不会开洞（自环抵消），留着只会污染法向与闭合性检查。
pub fn is_degenerate_triangle(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> bool {
    fn sub(p: [f32; 3], q: [f32; 3]) -> [f64; 3] {
        [
            p[0] as f64 - q[0] as f64,
            p[1] as f64 - q[1] as f64,
            p[2] as f64 - q[2] as f64,
        ]
    }
    fn d2(p: [f32; 3], q: [f32; 3]) -> f64 {
        let d = sub(p, q);
        d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
    }
    let eps2 = (WELD_EPS as f64) * (WELD_EPS as f64);
    if d2(a, b) <= eps2 || d2(b, c) <= eps2 || d2(c, a) <= eps2 {
        return true;
    }
    // f64 零面积（在 f32 顶点上用 f64 叉乘算——两端一致）。
    let (ab, ac) = (sub(b, a), sub(c, a));
    let cross = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    let len = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
    !(len > 0.0)
}

/// 丢掉退化三角形后的索引（顶点表不变，允许残留孤立顶点——不影响体积 /
/// 三角数 / 有效包围盒）。
pub fn drop_degenerate_tris(positions: &[[f32; 3]], indices: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(indices.len());
    for face in indices.chunks_exact(3) {
        let (i, j, k) = (face[0] as usize, face[1] as usize, face[2] as usize);
        if i >= positions.len() || j >= positions.len() || k >= positions.len() {
            continue;
        }
        if is_degenerate_triangle(positions[i], positions[j], positions[k]) {
            continue;
        }
        out.extend([face[0], face[1], face[2]]);
    }
    out
}

/// 展开成 Bevy `TriangleList`：每三角形三个顶点 + 平面法线（预览网格不需要光顺法线）。
pub fn triangle_list(data: &MeshData) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let mut positions = Vec::with_capacity(data.indices.len());
    let mut normals = Vec::with_capacity(data.indices.len());
    for face in data.indices.chunks_exact(3) {
        let (a, b, c) = (
            data.positions[face[0] as usize],
            data.positions[face[1] as usize],
            data.positions[face[2] as usize],
        );
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let cross = [
            ab[1] * ac[2] - ab[2] * ac[1],
            ab[2] * ac[0] - ab[0] * ac[2],
            ab[0] * ac[1] - ab[1] * ac[0],
        ];
        let len = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
        let n = if len > 0.0 {
            [cross[0] / len, cross[1] / len, cross[2] / len]
        } else {
            [0.0, 0.0, 1.0]
        };
        positions.extend([a, b, c]);
        normals.extend([n, n, n]);
    }
    (positions, normals)
}

/// 网格内容的稳定键（FNV-1a 64，按 f32 / u32 位模式）。同一份网格无论出现在哪个
/// 实例实体上都共享后端缓存。
pub fn mesh_key(data: &MeshData) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |bytes: &[u8]| {
        for &b in bytes {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for p in &data.positions {
        mix(&p[0].to_bits().to_le_bytes());
        mix(&p[1].to_bits().to_le_bytes());
        mix(&p[2].to_bits().to_le_bytes());
    }
    for i in &data.indices {
        mix(&i.to_le_bytes());
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube_mesh() -> MeshData {
        // 1×1×1 箱，原点为角点；每个面两个三角形，按面复制顶点（硬边口径）。
        let p: [[f32; 3]; 8] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ];
        let faces = [
            [0, 1, 2, 0, 2, 3], // -z
            [4, 6, 5, 4, 7, 6], // +z
            [0, 5, 1, 0, 4, 5], // -y? whatever, closed box
            [2, 7, 3, 2, 6, 7],
            [1, 6, 2, 1, 5, 6],
            [0, 3, 7, 0, 7, 4],
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

    #[test]
    fn weld_collapses_per_face_duplicated_vertices() {
        let mesh = cube_mesh();
        let (props, tri) = weld_mesh(&mesh.positions, &mesh.indices);
        assert_eq!(props.len() / 3, 8);
        assert_eq!(tri.len(), mesh.indices.len());
        assert!(tri.iter().all(|&i| (i as usize) < 8));
    }

    #[test]
    fn negative_zero_welds_with_zero() {
        let a = weld_key(-0.0, 0.0, 1.0);
        let b = weld_key(0.0, 0.0, 1.0);
        assert_eq!(a, b);
    }

    #[test]
    fn degenerate_triangle_is_dropped() {
        let positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]];
        // 第三个点与第一个重合——退化。
        let indices = drop_degenerate_tris(&positions, &[0, 1, 3]);
        assert!(indices.is_empty());
        // 正常三角形留下。
        let indices = drop_degenerate_tris(&positions, &[0, 1, 2]);
        assert_eq!(indices, vec![0, 1, 2]);
        // 共线（f64 零面积）也退化。
        let flat = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        assert!(drop_degenerate_tris(&flat, &[0, 1, 2]).is_empty());
    }

    #[test]
    fn triangle_list_expands_with_flat_normals() {
        let mesh = cube_mesh();
        let (positions, normals) = triangle_list(&mesh);
        assert_eq!(positions.len(), mesh.indices.len());
        assert_eq!(normals.len(), positions.len());
        // 每个三角形的三个法线相同。
        for tri in normals.chunks_exact(3) {
            assert_eq!(tri[0], tri[1]);
            assert_eq!(tri[1], tri[2]);
        }
    }

    #[test]
    fn mesh_key_depends_on_content() {
        let mesh = cube_mesh();
        assert_eq!(mesh_key(&mesh), mesh_key(&mesh.clone()));
        let mut other = mesh.clone();
        other.positions[0][0] += 1.0;
        assert_ne!(mesh_key(&mesh), mesh_key(&other));
    }
}
