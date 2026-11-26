//! 组件画廊里的「三维数据提资」场景（`--gallery=s15…`）：不接后端，把新窗的各个形态直接摆出来，
//! 对着画板 S15 族看（`design/三维数据提资.pen`）。
//!
//! 窗口发出的检查、更新、预览、提交都由这里按场景作答——与演示模式（`sim.rs`）是同一个思路，只是不起 Bevy、
//! 没有三维、也不用点：场景名定下来的那一刻画面就停在要看的形态上。在这里点「更新到最新」「提交」也走得通，
//! 但什么都不会真的发生。
//!
//! | 场景名 | 形态 | 画板 |
//! |---|---|---|
//! | `s15` | 需要更新（一个库落后、一个库需初始化） | S15 |
//! | `s15-a` | 更新中（增量在跑、首次导入在排） | S15-A |
//! | `s15-import` | 首次导入在跑，带解析保存进度 | S15-C |
//! | `s15-b` | 更新落定、已是最新，带本次更新结果 | S15-B |
//! | `s15-stuck` | 有库更新不了（文件回退、够不着、上次更新失败、元素已删除） | S15-C |
//! | `s15-unknown` | 没查成 | S15-C |
//! | `s15-unsupported` | 这台模型服务不支持提资前检查 | S15-C |
//! | `s15-empty` | 还没有提资元素 | — |
//! | `s15-bar` | 收成一条 | — |

use chrono::{Duration, Local};
use eframe::egui;
use plant_ui::data_handover::{self, Action, State};
use plant_ui::data_publish::{DesignPhase, PublishElement, PublishPreviewData};
use plant_ui::data_publish_check::{
    ElementCheck, ElementFact, NodeIngestBlock, NodeIngestFailure, NodeIngestLib, NodeLibrary,
    NodeModelStatus, Outcome, UNSUPPORTED_NO_ROUTE, settle,
};
use plant_ui::model_update::{Enqueued, EnqueuedBatch, FileAnomaly};
use plant_ui::style::tokens::{Density, Tokens};
use plant_ui::task_queue::{self, DataProgress, DbnumStatus, KIND_DATA_BATCH, QueueRow, TaskEntry};
use plant_ui::vm::{Selection, WorkbenchVm};
use plant_ui::{Cmd, RefU64};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    NeedsUpdate,
    Updating,
    Importing,
    Ready,
    Stuck,
    Unknown,
    Unsupported,
    Empty,
    Collapsed,
}

impl Scene {
    /// 全部场景名，认不出时列给人看。
    pub const NAMES: [&'static str; 9] = [
        "s15",
        "s15-a",
        "s15-import",
        "s15-b",
        "s15-stuck",
        "s15-unknown",
        "s15-unsupported",
        "s15-empty",
        "s15-bar",
    ];

    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "s15" => Self::NeedsUpdate,
            "s15-a" => Self::Updating,
            "s15-import" => Self::Importing,
            "s15-b" => Self::Ready,
            "s15-stuck" => Self::Stuck,
            "s15-unknown" => Self::Unknown,
            "s15-unsupported" => Self::Unsupported,
            "s15-empty" => Self::Empty,
            "s15-bar" => Self::Collapsed,
            _ => return None,
        })
    }
}

/// 「更新中」「已是最新」两个场景是演出来的：先按「需要更新」答一次，再替人点一下「更新到最新」，
/// 这样画出来的是窗口自己走到那一步的样子，而不是硬摆的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Before,
    Running { frames: u32 },
    After,
}

/// 一个提资元素在场景里的去向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Home {
    Db(u32),
    /// 文件最新保存里已经没有它。
    Gone,
    /// 没查成。
    Failed,
}

pub struct Demo {
    scene: Scene,
    stage: Stage,
    state: State,
    queue: task_queue::Vm,
    workbench: WorkbenchVm,
    elements: Vec<(RefU64, Home)>,
    checks: u32,
}

fn refno(a: u32, b: u32) -> RefU64 {
    RefU64::from_two_nums(a, b)
}

const TASK_INCREMENT: &str = "gallery-db8000";
const TASK_IMPORT: &str = "gallery-db8021";

impl Demo {
    pub fn new(scene: Scene) -> Self {
        let rows: Vec<(RefU64, &str, Home)> = match scene {
            Scene::Empty => Vec::new(),
            Scene::Stuck => vec![
                (refno(24390, 101), "/1WCC5101/B1", Home::Db(8003)),
                (refno(24391, 55), "/1WCC6200/B1", Home::Db(7015)),
                (refno(24392, 77), "/1WCC6301/B2", Home::Db(8004)),
                (refno(24383, 10412), "/1WCC2210/B1", Home::Db(7999)),
                (refno(24399, 9), "/1WCC7000/B9", Home::Gone),
            ],
            _ => vec![
                (refno(24384, 68341), "/1WCC0302A/B1", Home::Db(8000)),
                (refno(24384, 68355), "/1WCC0302A/B2", Home::Db(8000)),
                (refno(24384, 66458), "/1WCC1135/B1", Home::Db(8000)),
                (refno(24383, 10412), "/1WCC2210/B1", Home::Db(7999)),
                (
                    refno(24405, 2231),
                    "/1WCC4407/B1",
                    if scene == Scene::Unknown {
                        Home::Failed
                    } else {
                        Home::Db(8021)
                    },
                ),
            ],
        };

        let mut state = State::default();
        state.request_open();
        state.collapsed = scene == Scene::Collapsed;
        state.form.request.title = "1WCC 工艺管道提资 · 第 3 批".to_owned();
        state.form.request.design_phase = DesignPhase::Detailed;
        state.form.request.elements = rows
            .iter()
            .map(|(refno, name, _)| PublishElement {
                refno: *refno,
                name: (*name).to_owned(),
            })
            .collect();

        let mut queue = task_queue::Vm::new(Default::default());
        queue.project = "AMS-8009".to_owned();
        queue.mdb = "/ALL".to_owned();
        queue.namespace = "ams".to_owned();
        queue.loaded = true;

        let mut demo = Self {
            scene,
            stage: Stage::Before,
            state,
            queue,
            // 树上选着一个还不在列表里的元素：「添加」是亮的，「当前选中」那一行有话可说。
            workbench: WorkbenchVm {
                selection: Selection::single(refno(24384, 68343)),
                ..Default::default()
            },
            elements: rows
                .into_iter()
                .map(|(refno, _, home)| (refno, home))
                .collect(),
            checks: 0,
        };
        demo.install_queue();
        demo
    }

    pub fn show(&mut self, ctx: &egui::Context, t: &Tokens, d: Density) {
        // 画廊里这扇窗关不掉：关了就什么都没得看。
        self.state.open = true;
        let cmds = data_handover::show(ctx, t, d, &self.workbench, &self.queue, &mut self.state);
        for cmd in cmds {
            self.answer(cmd);
        }
        match (self.scene, self.stage) {
            // 第一次检查答完，替人点一下「更新到最新」。
            (Scene::Updating | Scene::Ready, Stage::Before) if self.checks > 0 => {
                let cmd = self.state.begin_update(&[8000], &[8021]);
                self.answer(cmd);
            }
            // 「已是最新」：让两条任务在队列里露几帧面，再一起跑完。
            (Scene::Ready, Stage::Running { frames }) if frames >= 3 => {
                self.stage = Stage::After;
                self.install_queue();
            }
            (_, Stage::Running { frames }) => self.stage = Stage::Running { frames: frames + 1 },
            _ => {}
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }

    /// 替后端答窗口发出的动作。
    fn answer(&mut self, cmd: Cmd) {
        let Cmd::DataHandover(action) = cmd else {
            return;
        };
        match action {
            Action::Open => self.state.request_open(),
            Action::Check { epoch, refnos } => {
                let outcome = self.outcome(&refnos);
                self.state.adopt_check(epoch, outcome);
                self.checks += 1;
            }
            Action::Update {
                lagging,
                needs_init,
            } => {
                let receipt = |dbnums: &[u32]| Enqueued {
                    enqueued: dbnums
                        .iter()
                        .enumerate()
                        .map(|(i, dbnum)| EnqueuedBatch {
                            task_id: format!("gallery-db{dbnum}"),
                            dbnum: *dbnum,
                            position: i + 1,
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
                };
                self.state.update_receipt(&receipt(&lagging));
                self.state.update_receipt(&receipt(&needs_init));
                self.state.update_queued();
                self.stage = Stage::Running { frames: 0 };
                self.install_queue();
            }
            Action::Preview(_) => self.state.adopt_preview(Ok(PublishPreviewData::default())),
            Action::LookupDeletions(_) => self.state.adopt_deletions(Ok(Vec::new())),
            Action::Publish(_) => self
                .state
                .adopt_published(Ok("画廊场景：已受理（未真正提交）".to_owned())),
        }
    }

    fn outcome(&self, refnos: &[RefU64]) -> Outcome {
        if self.scene == Scene::Unsupported {
            return Outcome::Unsupported(UNSUPPORTED_NO_ROUTE.to_owned());
        }
        let elements = refnos
            .iter()
            .map(|refno| {
                let home = self
                    .elements
                    .iter()
                    .find(|(known, _)| known == refno)
                    .map(|(_, home)| *home);
                ElementCheck {
                    refno: *refno,
                    fact: match home {
                        Some(Home::Db(dbnum)) => ElementFact::Checked(Box::new(self.status(dbnum))),
                        Some(Home::Failed) => {
                            ElementFact::Failed("请求模型服务失败：连接超时".to_owned())
                        }
                        // 场景之外新加进来的元素也归这一档：剧本里没有它。
                        Some(Home::Gone) | None => ElementFact::Gone,
                    },
                }
            })
            .collect();
        settle(elements, Local::now().format("%H:%M:%S").to_string())
    }

    /// 一个库此刻该答的节点状态。
    fn status(&self, dbnum: u32) -> NodeModelStatus {
        let lib = |verdict: &str, applied: i64, latest: i64, saved_at: &str| NodeModelStatus {
            dbnum,
            verdict: verdict.to_owned(),
            library: NodeLibrary {
                file_latest_sesno: latest,
                file_latest_time: Some(saved_at.to_owned()),
                applied_sesno: applied,
                initialized: applied > 0,
                model_verdict: Some(if applied > 0 { "in_sync" } else { "not_judged" }.to_owned()),
                ..Default::default()
            },
            ..Default::default()
        };
        match dbnum {
            8000 => {
                let saved_at = "2026-11-26T10:42:00+08:00";
                match (self.scene, self.stage) {
                    (Scene::Importing, _) | (_, Stage::After) => lib("latest", 92, 92, saved_at),
                    (_, Stage::Running { .. }) => lib("updating", 92, 92, saved_at),
                    (_, Stage::Before) => lib("latest", 89, 92, saved_at),
                }
            }
            8021 => {
                let saved_at = "2026-11-20T16:08:00+08:00";
                match self.stage {
                    Stage::After => lib("latest", 12, 12, saved_at),
                    _ => lib("not_generated", 0, 12, saved_at),
                }
            }
            // 文件回退：文件最新保存早于数据水位。
            8003 => lib("latest", 1005, 812, "2026-07-01T10:00:00+08:00"),
            7015 => lib("not_generated", 0, 5, "2026-10-02T08:30:00+08:00"),
            8004 => {
                let mut status = lib("latest", 70, 74, "2026-11-26T09:20:00+08:00");
                status.data_ingest = Some(NodeIngestBlock {
                    enabled: true,
                    libraries: vec![NodeIngestLib {
                        dbnum,
                        verdict: "present".to_owned(),
                        action: "none".to_owned(),
                        last_failure: Some(NodeIngestFailure {
                            reason: "读库文件失败：第 3 页校验和不对".to_owned(),
                            parked: true,
                        }),
                        ..Default::default()
                    }],
                });
                status
            }
            _ => lib("latest", 40, 40, "2026-11-24T09:15:00+08:00"),
        }
    }

    /// 按场景与阶段把队列快照摆好：库状态行、在排在跑的行、任务表。
    fn install_queue(&mut self) {
        let db = |dbnum: u32, applied: i32, latest: i32| DbnumStatus {
            dbnum,
            db_type: "DESI".to_owned(),
            file_name: format!("ams{dbnum}_0001"),
            applied_sesno: applied,
            file_latest_sesno: latest,
            initialized: applied > 0,
            ..Default::default()
        };
        let mut rolled_back = db(8003, 1005, 812);
        rolled_back.anomaly = Some(FileAnomaly::Rollback {
            file_latest_sesno: 812,
            applied_sesno: 1005,
            file_latest_sesno_time: Some("2026-07-01T10:00:00+08:00".to_owned()),
            applied_sesno_time: Some("2026-08-05T18:24:00+08:00".to_owned()),
        });
        rolled_back.blocked = true;
        let mut unreachable = db(7015, 0, 5);
        unreachable.not_in_project = true;
        unreachable.file_name.clear();
        self.queue.dbnums = vec![
            db(7999, 40, 40),
            db(8000, 89, 92),
            db(8021, 0, 12),
            rolled_back,
            unreachable,
            db(8004, 70, 74),
            // 提资元素之外还落后着的库：只进脚注的计数，不在这次更新里。
            db(8100, 10, 12),
            db(8101, 31, 32),
            db(8102, 7, 9),
            db(8103, 0, 4),
        ];

        let ago = |seconds: i64| (Local::now() - Duration::seconds(seconds)).to_rfc3339();
        let row = |task_id: &str, dbnum: u32, state: &str| QueueRow {
            task_id: task_id.to_owned(),
            dbnum,
            db_type: "DESI".to_owned(),
            state: state.to_owned(),
            start_sesno: 1,
            end_sesno: 2,
        };
        let task = |task_id: &str, dbnum: u32, state: &str| TaskEntry {
            task_id: task_id.to_owned(),
            kind: KIND_DATA_BATCH.to_owned(),
            state: state.to_owned(),
            project: "AMS-8009".to_owned(),
            dbnum: Some(dbnum),
            db_type: Some("DESI".to_owned()),
            ..Default::default()
        };
        (self.queue.queue.rows, self.queue.tasks) = match (self.scene, self.stage) {
            (Scene::Importing, _) => (
                vec![row(TASK_IMPORT, 8021, "running")],
                vec![TaskEntry {
                    created_at: ago(171),
                    started_at: Some(ago(153)),
                    data_progress: Some(DataProgress {
                        done: 123_456,
                        total: Some(250_000),
                    }),
                    ..task(TASK_IMPORT, 8021, "running")
                }],
            ),
            (_, Stage::Running { .. }) => (
                vec![
                    row(TASK_INCREMENT, 8000, "running"),
                    row(TASK_IMPORT, 8021, "queued"),
                ],
                vec![
                    TaskEntry {
                        created_at: ago(90),
                        started_at: Some(ago(72)),
                        units_done: Some(14),
                        total_units: Some(24),
                        ..task(TASK_INCREMENT, 8000, "running")
                    },
                    TaskEntry {
                        created_at: ago(18),
                        ..task(TASK_IMPORT, 8021, "queued")
                    },
                ],
            ),
            (_, Stage::After) => (
                Vec::new(),
                vec![
                    TaskEntry {
                        created_at: ago(600),
                        started_at: Some(ago(580)),
                        finished_at: Some(ago(520)),
                        units_done: Some(24),
                        total_units: Some(24),
                        ..task(TASK_INCREMENT, 8000, "succeeded")
                    },
                    TaskEntry {
                        created_at: ago(590),
                        started_at: Some(ago(520)),
                        finished_at: Some(ago(24)),
                        units_done: Some(412),
                        total_units: Some(412),
                        ..task(TASK_IMPORT, 8021, "succeeded")
                    },
                ],
            ),
            _ => (Vec::new(), Vec::new()),
        };
    }
}
