//! 浏览器端后端：`virtual-hole/worker.js`（module Worker）里跑官方 manifold-3d，
//! 主线程只管收发。焊接实体的 LRU 缓存与段数口径都在 Worker 侧（`boolean.js`），
//! 与原生端逐行对齐。
//!
//! 协议（均为普通 JS 对象，二进制用 transfer 收发）：
//! - 主 → Worker：`{type:'job', job:{jobId,epoch,meshKey,cutters:[{shape,size,affine,segments}]},
//!   positions?:Float32Array, indices?:Uint32Array}`；`{type:'forget', meshKey}`
//! - Worker → 主：`{type:'outcome', jobId, epoch, status:'Changed'|'NoIntersection'|'Failed'|'NeedMesh',
//!   positions?:Float32Array, indices?:Uint32Array, volumeBefore, volumeAfter, triCount, elapsedMs, error?}`
//!
//! `meshKey` 已带网格发过一次就不再重复传网格；Worker 缓存未命中回 `NeedMesh`，
//! 主线程去掉标记、带网格重提一次（作业本体留在 `pending` 里）。

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use js_sys::{Array, Float32Array, Object, Reflect, Uint32Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{MessageEvent, Worker, WorkerOptions, WorkerType};

use crate::{
    BooleanJob, BooleanOutcome, BooleanPreviewBackend, BooleanStatus, CutterShape, MeshData,
};

const WORKER_URL: &str = "virtual-hole/worker.js";

/// `Worker` 与 `Rc` 都不满足 `Send + Sync`，但后端要住进 Bevy 的 `Resource`。
/// wasm32-unknown-unknown 是单线程目标，这里 unsafe 放行是安全的。
struct SendSync<T>(T);
// SAFETY: wasm32 单线程，不存在跨线程共享访问。
unsafe impl<T> Send for SendSync<T> {}
// SAFETY: 同上。
unsafe impl<T> Sync for SendSync<T> {}

pub struct WebBackend {
    worker: SendSync<Worker>,
    /// Worker `onmessage` 推进来的结果。
    inbox: SendSync<Rc<RefCell<VecDeque<BooleanOutcome>>>>,
    /// 已经带网格发给 Worker 的 `mesh_key`。
    sent: HashSet<u64>,
    /// 等结果的作业（含网格，`NeedMesh` 重提要用）。
    pending: HashMap<u64, BooleanJob>,
}

impl WebBackend {
    pub fn new() -> Option<Self> {
        let options = WorkerOptions::new();
        options.set_type(WorkerType::Module);
        let worker = Worker::new_with_options(WORKER_URL, &options).ok()?;
        let inbox: Rc<RefCell<VecDeque<BooleanOutcome>>> =
            Rc::new(RefCell::new(VecDeque::new()));
        let inbox2 = inbox.clone();
        let onmessage = Closure::wrap(Box::new(move |event: MessageEvent| {
            if let Some(outcome) = parse_outcome(event.data()) {
                inbox2.borrow_mut().push_back(outcome);
            }
        }) as Box<dyn FnMut(_)>);
        worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        // 与 Worker 同寿命，有意泄漏。
        onmessage.forget();
        Some(Self {
            worker: SendSync(worker),
            inbox: SendSync(inbox),
            sent: HashSet::new(),
            pending: HashMap::new(),
        })
    }

    fn send_job(&mut self, job: &BooleanJob) {
        let msg = Object::new();
        let _ = Reflect::set(&msg, &"type".into(), &"job".into());
        let meta = Object::new();
        let _ = Reflect::set(&meta, &"jobId".into(), &JsValue::from_f64(job.job_id as f64));
        let _ = Reflect::set(&meta, &"epoch".into(), &JsValue::from_f64(job.epoch as f64));
        let _ = Reflect::set(
            &meta,
            &"meshKey".into(),
            &JsValue::from_f64(job.mesh_key as f64),
        );
        let cutters = Array::new();
        for cutter in &job.cutters {
            let o = Object::new();
            let shape = match cutter.shape {
                CutterShape::Circle => "Circle",
                CutterShape::Rect => "Rect",
            };
            let _ = Reflect::set(&o, &"shape".into(), &JsValue::from_str(shape));
            let _ = Reflect::set(
                &o,
                &"size".into(),
                &Array::from_iter(cutter.size_mm.iter().map(|v| JsValue::from_f64(*v))),
            );
            let _ = Reflect::set(
                &o,
                &"affine".into(),
                &Array::from_iter(cutter.affine.iter().map(|v| JsValue::from_f64(*v))),
            );
            let _ = Reflect::set(
                &o,
                &"segments".into(),
                &JsValue::from_f64(cutter.segments as f64),
            );
            cutters.push(&o);
        }
        let _ = Reflect::set(&meta, &"cutters".into(), &cutters);
        let _ = Reflect::set(&msg, &"job".into(), &meta);
        // 缓存里已有这个网格就不重复传。
        let transfer = Array::new();
        if let Some(mesh) = &job.mesh {
            if !self.sent.contains(&job.mesh_key) {
                let positions = Float32Array::new_with_length(mesh.positions.len() as u32 * 3);
                for (i, p) in mesh.positions.iter().enumerate() {
                    positions.set_index(i as u32 * 3, p[0]);
                    positions.set_index(i as u32 * 3 + 1, p[1]);
                    positions.set_index(i as u32 * 3 + 2, p[2]);
                }
                let indices = Uint32Array::new_with_length(mesh.indices.len() as u32);
                for (i, index) in mesh.indices.iter().enumerate() {
                    indices.set_index(i as u32, *index);
                }
                let _ = Reflect::set(&msg, &"positions".into(), &positions);
                let _ = Reflect::set(&msg, &"indices".into(), &indices);
                transfer.push(positions.buffer().as_ref());
                transfer.push(indices.buffer().as_ref());
                self.sent.insert(job.mesh_key);
            }
        }
        if let Err(error) = self.worker.0.post_message_with_transfer(&msg, &transfer) {
            web_sys::console::error_1(&format!("virtual-hole worker post failed: {error:?}").into());
        }
    }

    fn send_forget(&self, mesh_key: u64) {
        let msg = Object::new();
        let _ = Reflect::set(&msg, &"type".into(), &"forget".into());
        let _ = Reflect::set(&msg, &"meshKey".into(), &JsValue::from_f64(mesh_key as f64));
        if let Err(error) = self.worker.0.post_message(&msg) {
            web_sys::console::error_1(&format!("virtual-hole worker forget failed: {error:?}").into());
        }
    }
}

impl Drop for WebBackend {
    fn drop(&mut self) {
        self.worker.0.terminate();
    }
}

impl BooleanPreviewBackend for WebBackend {
    fn submit(&mut self, job: BooleanJob) {
        self.send_job(&job);
        self.pending.insert(job.job_id, job);
    }
    fn poll(&mut self) -> Vec<BooleanOutcome> {
        let mut out = Vec::new();
        loop {
            // 每次只弹一个：`RefMut` 临时不能活过 `send_job`（要 &mut self）。
            let outcome = self.inbox.0.borrow_mut().pop_front();
            let Some(outcome) = outcome else { break };
            if outcome.status == BooleanStatus::NeedMesh {
                // 缓存未命中：去掉标记，带网格重提一次。
                if let Some(job) = self.pending.get(&outcome.job_id) {
                    let job = job.clone();
                    self.sent.remove(&job.mesh_key);
                    self.send_job(&job);
                    continue;
                }
            }
            self.pending.remove(&outcome.job_id);
            out.push(outcome);
        }
        out
    }
    fn forget(&mut self, mesh_key: u64) {
        self.sent.remove(&mesh_key);
        self.send_forget(mesh_key);
    }
}

/// 解析 Worker 回包；不是 outcome 消息时返回 `None`。
fn parse_outcome(data: JsValue) -> Option<BooleanOutcome> {
    let obj: Object = data.dyn_into().ok()?;
    let ty = Reflect::get(&obj, &"type".into())
        .ok()?
        .as_string()?;
    if ty != "outcome" {
        return None;
    }
    let get_str = |key: &str| {
        Reflect::get(&obj, &key.into())
            .ok()
            .and_then(|v| v.as_string())
    };
    let get_num = |key: &str| Reflect::get(&obj, &key.into()).ok().and_then(|v| v.as_f64());
    let status = match get_str("status").as_deref() {
        Some("Changed") => BooleanStatus::Changed,
        Some("NoIntersection") => BooleanStatus::NoIntersection,
        Some("Failed") => BooleanStatus::Failed,
        Some("NeedMesh") => BooleanStatus::NeedMesh,
        _ => return None,
    };
    let mesh = if status == BooleanStatus::Changed {
        let positions: Float32Array = Reflect::get(&obj, &"positions".into())
            .ok()?
            .dyn_into()
            .ok()?;
        let indices: Uint32Array = Reflect::get(&obj, &"indices".into())
            .ok()?
            .dyn_into()
            .ok()?;
        let mut p = Vec::with_capacity(positions.length() as usize / 3);
        for i in 0..positions.length() / 3 {
            p.push([
                positions.get_index(i * 3),
                positions.get_index(i * 3 + 1),
                positions.get_index(i * 3 + 2),
            ]);
        }
        let mut idx = Vec::with_capacity(indices.length() as usize);
        for i in 0..indices.length() {
            idx.push(indices.get_index(i));
        }
        Some(MeshData {
            positions: p,
            indices: idx,
        })
    } else {
        None
    };
    Some(BooleanOutcome {
        job_id: get_num("jobId")? as u64,
        epoch: get_num("epoch")? as u64,
        status,
        mesh,
        volume_before: get_num("volumeBefore").unwrap_or(0.0),
        volume_after: get_num("volumeAfter").unwrap_or(0.0),
        tri_count: get_num("triCount").unwrap_or(0.0) as usize,
        elapsed_ms: get_num("elapsedMs").unwrap_or(0.0) as u64,
        error: get_str("error"),
    })
}
