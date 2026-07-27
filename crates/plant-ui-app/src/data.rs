//! UI 线程与数据线程（tokio）之间的桥。
//!
//! eframe 的绘制是同步的，而 SUL_DB 全是 async：这里起一个常驻数据线程，
//! 请求经 channel 进、结果经 channel 出，UI 每帧非阻塞地收。每个结果落地后
//! `request_repaint`，免得空闲中的 UI 睡过结果。

use std::sync::mpsc;

use plant_ui::model_update::{Accepted, Preview, Run};
use plant_ui_data::{EleTreeNode, RefU64};

pub enum Req {
    /// 懒加载某节点的直接子层。
    Children(RefU64),
    /// 选中元素的 UI 属性表。
    Props(RefU64),
    /// 重跑启动序列。连库失败后命令行上的「重试」走这条，结果仍走 `Evt::Ready`。
    Reconnect,
    ModelUpdatePreview {
        base: String,
        project: String,
    },
    ModelUpdateExecute {
        base: String,
        project: String,
        dbnums: Vec<u32>,
    },
    ModelUpdateTask {
        base: String,
        run_id: String,
    },
}

/// 启动序列（连接 + 工程标识 + SITE 根层）的合并产物。
pub struct ReadyInfo {
    pub project: String,
    pub ns: String,
    pub db_nums: Vec<u32>,
    pub sites: Vec<EleTreeNode>,
}

pub enum Evt {
    Ready(anyhow::Result<ReadyInfo>),
    Children(RefU64, anyhow::Result<Vec<EleTreeNode>>),
    Props(RefU64, anyhow::Result<Vec<plant_ui_data::Attr>>),
    ModelUpdatePreview(anyhow::Result<Preview>),
    ModelUpdateExecute(anyhow::Result<Accepted>),
    ModelUpdateTask(String, anyhow::Result<Run>),
}

pub struct Bridge {
    pub req: mpsc::Sender<Req>,
    pub evt: mpsc::Receiver<Evt>,
}

/// 启动序列：连库、抓工程标识、抓 SITE 根层。三步任一失败都算没连上。
async fn ready() -> anyhow::Result<ReadyInfo> {
    plant_ui_data::connect().await?;
    let (project, ns, db_nums) = plant_ui_data::project_identity().await?;
    let sites = plant_ui_data::site_nodes().await?;
    Ok(ReadyInfo {
        project,
        ns,
        db_nums,
        sites,
    })
}

/// 起数据线程：连库、抓工程标识与 SITE 根层，然后循环处理懒加载请求。
pub fn spawn(ctx: egui::Context) -> Bridge {
    let (req_tx, req_rx) = mpsc::channel();
    let (evt_tx, evt_rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("plant-ui-data".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(async move {
                let _ = evt_tx.send(Evt::Ready(ready().await));
                ctx.request_repaint();

                while let Ok(req) = req_rx.recv() {
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
                        Req::ModelUpdatePreview { base, project } => {
                            let tx = evt_tx.clone();
                            let repaint = ctx.clone();
                            std::thread::spawn(move || {
                                let _ = tx.send(Evt::ModelUpdatePreview(
                                    crate::model_update_api::preview(&base, &project),
                                ));
                                repaint.request_repaint();
                            });
                        }
                        Req::ModelUpdateExecute {
                            base,
                            project,
                            dbnums,
                        } => {
                            let tx = evt_tx.clone();
                            let repaint = ctx.clone();
                            std::thread::spawn(move || {
                                let _ = tx.send(Evt::ModelUpdateExecute(
                                    crate::model_update_api::execute(&base, &project, &dbnums),
                                ));
                                repaint.request_repaint();
                            });
                        }
                        Req::ModelUpdateTask { base, run_id } => {
                            let tx = evt_tx.clone();
                            let repaint = ctx.clone();
                            std::thread::spawn(move || {
                                let result = crate::model_update_api::task(&base, &run_id);
                                let _ = tx.send(Evt::ModelUpdateTask(run_id, result));
                                repaint.request_repaint();
                            });
                        }
                    }
                }
            });
        })
        .expect("spawn data thread");
    Bridge {
        req: req_tx,
        evt: evt_rx,
    }
}
