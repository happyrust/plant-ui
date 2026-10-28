//! 黄金对拍夹具（ADR-0031）：固定几何喂给真实原生后端，结果录进
//! `tests/fixtures/parity.json`；浏览器端靠 `scripts/virtual-hole-parity.mjs`
//! 拿同一份输入跑官方 manifold-3d 对拍（三角数完全一致、体积相对误差 < 1e-6、
//! 包围盒误差 < 0.02mm，见 web/public/virtual-hole/VENDOR.md）。
//!
//! 夹具缺失时按当前结果写一份（黄金模式，输出里有提示）；夹具在时比对：
//! 状态一致、三角数完全一致、体积相对误差 < 1e-9、包围盒误差 < 1e-3mm
//! （原生端自己是确定性的，口径比对拍脚本收紧）。
//!
//! 跑法：`cargo test -p plant-csg --features manifold --test parity`

#![cfg(all(not(target_arch = "wasm32"), feature = "manifold"))]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use plant_csg::{
    BooleanJob, BooleanOutcome, CutterJob, CutterShape, MeshData, backend, mesh_key,
};
use serde_json::{Value, json};

/// 黄金夹具路径（相对 crate 根，测试的工作目录就是这里）。
fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/parity.json")
}

/// 1000×600×400 的箱子（毫米），一个角在原点。
fn box_mesh() -> MeshData {
    let (w, h, d) = (1000.0f32, 600.0, 400.0);
    let p: Vec<[f32; 3]> = (0..8)
        .map(|i| {
            [
                if i & 1 == 0 { 0.0 } else { w },
                if i & 2 == 0 { 0.0 } else { h },
                if i & 4 == 0 { 0.0 } else { d },
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
    for face in faces {
        let base = positions.len() as u32;
        for &v in &face {
            positions.push(p[v]);
        }
        indices.extend([base, base + 1, base + 2, base + 3, base + 4, base + 5]);
    }
    MeshData { positions, indices }
}

/// 膨胀口径（libgm `restol`）：与 `plant_ui::virtual_hole::NEGATIVE_INFLATE_MM` 同值，
/// 上游 `VirtualHole::cutter_mat4` 在仿射里已经折进尺寸——这里照抄同一口径。
const INFLATE_MM: f64 = 0.051;

/// 列主序 4×3 的 `T(x,y,z) · S(sx,sy,sz)`——与 `VirtualHole::cutter_mat4` 同一口径
/// （旋转恒等，尺寸已膨胀）。
fn scale_translate_affine(sx: f64, sy: f64, sz: f64, x: f64, y: f64, z: f64) -> [f64; 12] {
    [sx, 0.0, 0.0, 0.0, sy, 0.0, 0.0, 0.0, sz, x, y, z]
}

fn circle_cutter(x: f64, y: f64, z: f64, diameter: f64, depth: f64, segments: u32) -> CutterJob {
    let d = diameter + 2.0 * INFLATE_MM;
    let h = depth + 2.0 * INFLATE_MM;
    CutterJob {
        shape: CutterShape::Circle,
        size_mm: [d, d, h],
        affine: scale_translate_affine(d, d, h, x, y, z),
        segments,
    }
}

fn rect_cutter(x: f64, y: f64, z: f64, width: f64, height: f64, depth: f64) -> CutterJob {
    let w = width + 2.0 * INFLATE_MM;
    let h = height + 2.0 * INFLATE_MM;
    let d = depth + 2.0 * INFLATE_MM;
    CutterJob {
        shape: CutterShape::Rect,
        size_mm: [w, h, d],
        affine: scale_translate_affine(w, h, d, x, y, z),
        segments: 0,
    }
}

struct Case {
    name: &'static str,
    mesh: MeshData,
    cutters: Vec<CutterJob>,
}

/// 固定的对拍用例：完全穿透、多孔洞同实体、无相交。
fn cases() -> Vec<Case> {
    let box_mesh = box_mesh();
    vec![
        Case {
            name: "through_circle",
            mesh: box_mesh.clone(),
            // Φ325 圆孔沿 Z 贯穿 400 厚的箱子（切削体长 800，两头冒出去）。
            cutters: vec![circle_cutter(500.0, 300.0, 200.0, 325.0, 800.0, 96)],
        },
        Case {
            name: "circle_and_rect",
            mesh: box_mesh.clone(),
            // 圆孔 + 矩形孔落在同一个实体上，合并成一次 difference。
            cutters: vec![
                circle_cutter(300.0, 300.0, 200.0, 325.0, 800.0, 96),
                rect_cutter(700.0, 300.0, 200.0, 300.0, 400.0, 800.0),
            ],
        },
        Case {
            name: "far_away",
            mesh: box_mesh,
            // 切削体离箱子十万八千里：无相交，不许提交作业以外的副作用。
            cutters: vec![circle_cutter(5000.0, 5000.0, 200.0, 325.0, 800.0, 96)],
        },
    ]
}

/// 跑满一个用例：提交给真实后端，转圈等结果。
fn run_case(case: &Case) -> BooleanOutcome {
    let mut backend = backend();
    backend.submit(BooleanJob {
        job_id: 1,
        epoch: 1,
        mesh_key: mesh_key(&case.mesh),
        mesh: Some(Arc::new(case.mesh.clone())),
        cutters: case.cutters.clone(),
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        // 一个用例一个作业：取第一条结果即可（`for ... return` 会被 clippy 判成 never_loop）。
        if let Some(outcome) = backend.poll().into_iter().next() {
            return outcome;
        }
        assert!(Instant::now() < deadline, "后端 30s 没回结果");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// 结果网格的包围盒。
fn bbox(mesh: &MeshData) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for p in &mesh.positions {
        for axis in 0..3 {
            min[axis] = min[axis].min(p[axis]);
            max[axis] = max[axis].max(p[axis]);
        }
    }
    (min, max)
}

fn expected_of(outcome: &BooleanOutcome) -> Value {
    let (bbox_min, bbox_max) = match &outcome.mesh {
        Some(mesh) => {
            let (min, max) = bbox(mesh);
            (Some(min), Some(max))
        }
        None => (None, None),
    };
    json!({
        "status": format!("{:?}", outcome.status),
        "triCount": outcome.tri_count,
        "volumeBefore": outcome.volume_before,
        "volumeAfter": outcome.volume_after,
        "bboxMin": bbox_min,
        "bboxMax": bbox_max,
        "error": outcome.error,
    })
}

fn case_record(case: &Case, outcome: &BooleanOutcome) -> Value {
    json!({
        "name": case.name,
        "mesh": {
            "positions": case.mesh.positions,
            "indices": case.mesh.indices,
        },
        "cutters": case.cutters.iter().map(|cutter| json!({
            "shape": format!("{:?}", cutter.shape),
            "size": cutter.size_mm,
            "affine": cutter.affine,
            "segments": cutter.segments,
        })).collect::<Vec<_>>(),
        "expected": expected_of(outcome),
    })
}

/// 相对误差。
fn rel_diff(a: f64, b: f64) -> f64 {
    (a - b).abs() / a.abs().max(b.abs()).max(1.0)
}

fn compare(case: &Case, expected: &Value, outcome: &BooleanOutcome) {
    assert_eq!(
        format!("{:?}", outcome.status),
        expected["status"].as_str().unwrap(),
        "{}: 状态不一致",
        case.name
    );
    assert_eq!(
        outcome.tri_count,
        expected["triCount"].as_u64().unwrap() as usize,
        "{}: 三角数不一致",
        case.name
    );
    assert!(
        rel_diff(outcome.volume_before, expected["volumeBefore"].as_f64().unwrap()) < 1e-9,
        "{}: volumeBefore 相对误差过大",
        case.name
    );
    assert!(
        rel_diff(outcome.volume_after, expected["volumeAfter"].as_f64().unwrap()) < 1e-9,
        "{}: volumeAfter 相对误差过大",
        case.name
    );
    if let Some(mesh) = &outcome.mesh {
        let (min, max) = bbox(mesh);
        let expected_min = expected["bboxMin"].as_array().unwrap();
        let expected_max = expected["bboxMax"].as_array().unwrap();
        for axis in 0..3 {
            assert!(
                (min[axis] - expected_min[axis].as_f64().unwrap() as f32).abs() < 1e-3,
                "{}: bboxMin[{axis}] 误差过大",
                case.name
            );
            assert!(
                (max[axis] - expected_max[axis].as_f64().unwrap() as f32).abs() < 1e-3,
                "{}: bboxMax[{axis}] 误差过大",
                case.name
            );
        }
    }
}

#[test]
fn parity_fixture_matches_native_backend() {
    let cases = cases();
    let path = fixture_path();
    if !path.exists() {
        // 黄金模式：按当前结果录夹具，给人工检查后再跑比对。
        let records: Vec<Value> = cases
            .iter()
            .map(|case| {
                let outcome = run_case(case);
                case_record(case, &outcome)
            })
            .collect();
        let fixture = json!({ "cases": records });
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(&path, serde_json::to_string_pretty(&fixture).unwrap()).unwrap();
        eprintln!("黄金模式：已写入夹具 {}", path.display());
        return;
    }
    let recorded: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let recorded_cases = recorded["cases"].as_array().unwrap();
    assert_eq!(
        recorded_cases.len(),
        cases.len(),
        "夹具用例数与代码里的用例数对不上（代码加了用例要重录夹具）"
    );
    for case in &cases {
        let expected = recorded_cases
            .iter()
            .find(|record| record["name"].as_str() == Some(case.name))
            .unwrap_or_else(|| panic!("夹具里没有用例 {}", case.name));
        let outcome = run_case(case);
        compare(case, &expected["expected"], &outcome);
    }
}
