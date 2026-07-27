//! UI 线程与数据线程（tokio）之间的桥。
//!
//! eframe 的绘制是同步的，而 SUL_DB 全是 async：这里起一个常驻数据线程，
//! 请求经 channel 进、结果经 channel 出，UI 每帧非阻塞地收。每个结果落地后
//! `request_repaint`，免得空闲中的 UI 睡过结果。

use std::sync::mpsc;

use plant_ui::model_update::{Enqueued, Preview, ProgressEvent, Run};
use plant_ui::task_queue::Poll as QueuePoll;
use plant_ui_data::{EleTreeNode, RefU64};

pub enum Req {
    /// 懒加载某节点的直接子层。
    Children(RefU64),
    /// 选中元素的 UI 属性表。
    Props(RefU64),
    /// 加载这些模型树根下已经生成的几何实例。
    Models(Vec<RefU64>),
    /// 命令行按名称定位元素。
    ResolveName(String),
    /// 重跑启动序列。连库失败后命令行上的「重试」走这条，结果仍走 `Evt::Ready`。
    Reconnect,
    ModelUpdatePreview {
        base: String,
        project: String,
        mdb: String,
    },
    ModelUpdateExecute {
        base: String,
        project: String,
        mdb: String,
    },
    ModelUpdateTask {
        base: String,
        run_id: String,
    },
    /// 重新生成一个交付单元。幂等，S4-C 的失败行上那枚「重试」走它。
    EnsureModel {
        base: String,
        root_refno: String,
    },
    /// 取回工作：丢缓存，重查 SITE 根层与这些已展开分支的子层。
    ///
    /// 分支由 App 侧算好交下来。数据线程不认识展开状态，也不该认识——它只是
    /// 「照这张单子重查一遍」。
    GetWork {
        branches: Vec<RefU64>,
    },
    /// 读一次设计库水位，给取回工作旁边那行提示用。
    PendingSessions,
    /// 队列面板的一次轮询（队列快照 + 任务表 + health + 持久欠账）。
    QueuePoll {
        base: String,
    },
    /// 暂停 / 恢复出队。
    QueueSetPaused {
        base: String,
        paused: bool,
    },
}

/// 一次取回工作重新查回来的那部分树。
pub struct GetWork {
    pub sites: Vec<EleTreeNode>,
    /// 重查成功的分支及其新的直接子层。
    pub branches: Vec<(RefU64, Vec<EleTreeNode>)>,
    /// 重查失败的分支及原因。一个分支查不动不该让整次取回作废，
    /// 它那一层就保持原样，失败单独进日志。
    pub failed: Vec<(RefU64, String)>,
}

/// 启动序列（连接 + 工程标识 + SITE 根层）的合并产物。
pub struct ReadyInfo {
    pub project: String,
    /// 当前 MDB 名（带前导 `/`）。模型更新用它定本期执行范围。
    pub mdb: String,
    pub ns: String,
    pub db_nums: Vec<u32>,
    pub sites: Vec<EleTreeNode>,
}

pub enum Evt {
    Ready(anyhow::Result<ReadyInfo>),
    Children(RefU64, anyhow::Result<Vec<EleTreeNode>>),
    Props(RefU64, anyhow::Result<Vec<plant_ui_data::Attr>>),
    Models(anyhow::Result<Vec<aios_core::GeomInstQuery>>),
    ResolvedName(String, anyhow::Result<Option<RefU64>>),
    ModelUpdatePreview(anyhow::Result<Preview>),
    /// 「扫描 + 入队」的回执。合流之后它不再是单个 task_id。
    ModelUpdateExecute(anyhow::Result<Enqueued>),
    ModelUpdateTask(String, anyhow::Result<Run>),
    /// 单元重生成的回执。成功与否只进命令行日志——终态摘要是服务端那份，
    /// 不在本端就地改写它。
    EnsureModel(String, anyhow::Result<crate::model_update_api::Ensured>),
    /// 取回工作的整批结果。根层查不动就是整次失败——根层没了树无从谈起。
    GetWork(anyhow::Result<GetWork>),
    /// 设计库水位。查不动就不显示那行提示，不值得为它报错。
    PendingSessions(anyhow::Result<u32>),
    /// 执行期的逐行明细。走 WebSocket，不经数据线程（见 `model_update_ws`）。
    ModelUpdateProgress(ProgressEvent),
    ModelUpdateFeedLive,
    ModelUpdateFeedDown(String),
    /// 队列面板的一次轮询结果。四份数据一起换代，不留自相矛盾的中间态。
    QueuePoll(anyhow::Result<QueuePoll>),
    /// 暂停 / 恢复的回执。真值仍以下一次轮询的快照为准，这里只负责把失败说出来。
    QueueSetPaused(bool, anyhow::Result<()>),
    /// 队列视图的逐单元明细，带发生在哪个任务上。
    QueueProgress(String, ProgressEvent),
    /// 有任务起讫。只当醒钟用：叫轮询早一拍去取，不拿它改行状态。
    QueueTaskChanged,
    QueueFeedLive,
    QueueFeedDown(String),
}

pub struct Bridge {
    pub req: mpsc::Sender<Req>,
    pub evt: mpsc::Receiver<Evt>,
    /// 给数据线程之外的生产者用（现在只有那条 WebSocket）。UI 每帧只认一个收端，
    /// 长连接自己另开一条 channel 的话就得在 `pump_events` 里再轮询一次。
    pub evt_tx: mpsc::Sender<Evt>,
}

/// 取回工作：先丢本进程的查询缓存，再把根层与这些分支重查一遍。
///
/// 缓存必须先丢。重查 SITE 根层走的是带 memoize 的那条查询，缓存还在的话它会
/// 原样把旧的那份还回来，界面看着刷新过了、内容一个字没变。
async fn get_work(branches: &[RefU64]) -> anyhow::Result<GetWork> {
    plant_ui_data::invalidate_all().await;
    let sites = plant_ui_data::site_nodes().await?;
    let mut loaded = Vec::with_capacity(branches.len());
    let mut failed = Vec::new();
    for refno in branches {
        match plant_ui_data::child_nodes((*refno).into()).await {
            Ok(kids) => loaded.push((*refno, kids)),
            Err(error) => failed.push((*refno, crate::logs::error_chain(&error))),
        }
    }
    Ok(GetWork {
        sites,
        branches: loaded,
        failed,
    })
}

/// 启动序列：连库、抓工程标识、抓 SITE 根层。三步任一失败都算没连上。
async fn ready() -> anyhow::Result<ReadyInfo> {
    plant_ui_data::connect().await?;
    let (project, mdb, ns, db_nums) = plant_ui_data::project_identity().await?;
    let sites = plant_ui_data::site_nodes().await?;
    Ok(ReadyInfo {
        project,
        mdb,
        ns,
        db_nums,
        sites,
    })
}

/// 起数据线程：连库、抓工程标识与 SITE 根层，然后循环处理懒加载请求。
pub fn spawn(ctx: egui::Context, tasks: &bevy_wasm_tasks::Tasks<'_>) -> Bridge {
    let (req_tx, req_rx) = mpsc::channel();
    let (evt_tx, evt_rx) = mpsc::channel();
    let evt_tx_out = evt_tx.clone();
    let worker = move |mut task_ctx: bevy_wasm_tasks::TaskContext| async move {
        let _ = evt_tx.send(Evt::Ready(ready().await));
        ctx.request_repaint();

        loop {
            while let Ok(req) = req_rx.try_recv() {
                match req {
                    Req::Reconnect => {
                        let _ = evt_tx.send(Evt::Ready(ready().await));
                        ctx.request_repaint();
                    }
                    Req::Children(refno) => {
                        let r = plant_ui_data::child_nodes(refno.into()).await;
                        let _ = evt_tx.send(Evt::Children(refno, r));
                        ctx.request_repaint();
                    }
                    Req::Props(refno) => {
                        let r = plant_ui_data::element_props(refno.into()).await;
                        let _ = evt_tx.send(Evt::Props(refno, r));
                        ctx.request_repaint();
                    }
                    Req::Models(roots) => {
                        let result = plant_ui_data::model_instances(&roots).await;
                        let _ = evt_tx.send(Evt::Models(result));
                        ctx.request_repaint();
                    }
                    Req::ResolveName(name) => {
                        let result = plant_ui_data::resolve_name(&name).await;
                        let _ = evt_tx.send(Evt::ResolvedName(name, result));
                        ctx.request_repaint();
                    }
                    Req::ModelUpdatePreview { base, project, mdb } => {
                        let result = crate::model_update_api::preview(&base, &project, &mdb).await;
                        let _ = evt_tx.send(Evt::ModelUpdatePreview(result));
                        ctx.request_repaint();
                    }
                    Req::ModelUpdateExecute { base, project, mdb } => {
                        let result = crate::model_update_api::execute(&base, &project, &mdb).await;
                        let _ = evt_tx.send(Evt::ModelUpdateExecute(result));
                        ctx.request_repaint();
                    }
                    Req::ModelUpdateTask { base, run_id } => {
                        let result = crate::model_update_api::task(&base, &run_id).await;
                        let _ = evt_tx.send(Evt::ModelUpdateTask(run_id, result));
                        ctx.request_repaint();
                    }
                    Req::GetWork { branches } => {
                        let result = get_work(&branches).await;
                        let _ = evt_tx.send(Evt::GetWork(result));
                        ctx.request_repaint();
                    }
                    Req::PendingSessions => {
                        let result = plant_ui_data::pending_sessions().await;
                        let _ = evt_tx.send(Evt::PendingSessions(result));
                        ctx.request_repaint();
                    }
                    Req::QueuePoll { base } => {
                        let result = crate::model_update_api::poll_queue(&base).await;
                        let _ = evt_tx.send(Evt::QueuePoll(result));
                        ctx.request_repaint();
                    }
                    Req::QueueSetPaused { base, paused } => {
                        let result = crate::model_update_api::set_queue_paused(&base, paused).await;
                        let _ = evt_tx.send(Evt::QueueSetPaused(paused, result));
                        ctx.request_repaint();
                    }
                    Req::EnsureModel { base, root_refno } => {
                        let result =
                            crate::model_update_api::ensure_model(&base, &root_refno).await;
                        let _ = evt_tx.send(Evt::EnsureModel(root_refno, result));
                        ctx.request_repaint();
                    }
                }
            }
            task_ctx.sleep_updates(1).await;
        }
    };
    #[cfg(target_arch = "wasm32")]
    tasks.spawn_wasm(worker);
    #[cfg(not(target_arch = "wasm32"))]
    tasks.spawn_tokio(worker);
    Bridge {
        req: req_tx,
        evt: evt_rx,
        evt_tx: evt_tx_out,
    }
}
