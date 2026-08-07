//! 进程内模拟引擎（方案 A）：`PLANT_UI_SIM=1` 时整个数据线程改由这里供数，
//! 不碰网络、不碰 SurrealDB、不要求 gen-model 在场。
//!
//! 引擎产出的就是现有契约结构体（`task_queue::Poll` / `model_update::Preview` /
//! `Enqueued` / `ProgressEvent`），UI 层一行不改——它分不出自己面对的是真服务
//! 还是剧本。逐单元明细本来走 WebSocket，这里由 [`Engine::pump`] 定时经
//! `evt_tx` 推同形的 `Evt::QueueProgress`，通道状态恒为 Live。
//!
//! 内置剧本对齐 S2 / S12 的主要状态：
//! - db7001：两个会话待应用，2 个交付单元 + 1 个纯位姿目标，全部成功；
//!   两个单元分处两个 SITE，S2-G 的同库联动勾选与「与 … 同库联动」详情行有得看；
//! - db7002：一个会话待应用，3 个单元中 PANE 生成失败 → partial → 死信，
//!   可在队列面板人工复活（复活后约 2.5 秒自愈消失）；
//!   排队期间还会被「新到会话」并入一次（merged_sesnos 展示吸收扩窗）；
//!   它还带一个跨 SITE 挪动的单元（两个桶里各出现一次）与一个「SITE 归属未知」桶；
//! - db7003：已是最新；db7004：文件回退 → 阻断不入队；
//! - db7005：MDB 声明但不在当前项目目录（够不着）；db7006：首次导入（需初始化），
//!   契约不为它解 SITE 桶，S2-G 走文件身份行的优雅降级分支；
//! - db8001：CATA，非 DESI 不进本期执行范围；
//! - 数据批次跑空后收一轮房间归属重算（泳道 Converging → Converged）。
//!
//! 时间一律用 `chrono`（wasm 上 `std::time::Instant` 不可用；虽然浏览器端读不到
//! 环境变量、引擎永远不会被构造，但这个模块要在两个目标上都编译过）。

use std::collections::HashMap;
use std::sync::OnceLock;

use chrono::{DateTime, Duration, SecondsFormat, Utc};

use plant_ui::model_update::{
    BatchResult, BatchStatus, BlockedDbnum, DbPreview, Enqueued, EnqueuedBatch, FileAnomaly,
    Outcome, PendingModelUnit, Preview, ProgressEvent, RunStatus, SessionPreview, SitePreview,
    TransformTargetPreview, UnitPreview, UnitResult, UnitStatus, ZonePreview,
};
use plant_ui::task_queue::{
    DbnumStatus, Health, KIND_DATA_BATCH, KIND_ROOM_RECALC, Poll, QueueRow, QueueSnapshot,
    RoomCounts, TaskEntry,
};
use plant_ui_data::{Attr, AttrKind, EleTreeNode, RefU64};

use crate::data::{Evt, GetWork, ReadyInfo};

/// 模拟身份。三个值同时喂给 `ready()` 与 `/health`，身份校验（S12 的
/// `identity_status`）走的是与真服务同一条比对路径。
const PROJECT: &str = "SIMPRJ";
const MDB: &str = "/SIM-ALL";
const NAMESPACE: &str = "sim";

/// 数据应用阶段（出队冻结 → 第一个单元开始）的时长。
const APPLY_MS: i64 = 3_000;
/// 每个交付单元的生成时长。
const UNIT_MS: i64 = 2_000;
/// 人工复活死信后到自愈消失的时长。
const RETRY_MS: i64 = 2_500;
/// 房间轮每收敛一个目标的间隔。
const ROOM_STEP_MS: i64 = 600;

pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("PLANT_UI_SIM")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
    })
}

fn stamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Secs, true)
}

// ================================================================ 剧本世界

/// 一个 SITE 桶的身份（ADR-020 `SiteSummary`）。`refno` 空串 = 「SITE 归属未知」，
/// §2-G 里它无勾选框、排在同库 SITE 行之后。
#[derive(Clone, Copy)]
struct SimSite {
    refno: &'static str,
    name: &'static str,
}

const fn site(refno: &'static str, name: &'static str) -> SimSite {
    SimSite { refno, name }
}

/// 剧本里的两个 SITE，与模型树的根层同名，看起来是一个世界。
const SITE_A: SimSite = site("9001/1", "/SIM-SITE-A");
const SITE_B: SimSite = site("9002/1", "/SIM-SITE-B");

/// 一个交付单元的脚本：失败消息为 `Some` 时按 partial 剧本走。
#[derive(Clone)]
struct SimUnit {
    root_refno: &'static str,
    noun: &'static str,
    name: &'static str,
    /// post 状态下它归哪个 SITE。`None` = 契约没为它解 SITE 桶——首次导入的库
    /// 没有 pre/post 窗口，就是这一种。
    site: Option<SimSite>,
    /// pre 状态下它归哪个 SITE——只有跨 SITE 挪过的单元才有。
    moved_from: Option<SimSite>,
    fail: Option<&'static str>,
}

/// 一个库的脚本状态。`applied` 会随批次完成推进，是引擎里唯一的「水位」。
struct SimDb {
    dbnum: u32,
    db_type: &'static str,
    file_name: &'static str,
    applied: i32,
    latest: i32,
    /// `applied` / `latest` 那两个会话在 **E3D 里被写入的时刻**（ADR-020），记成
    /// 「距剧本纪元几小时前」。`None` = 从未应用或读不到会话页，界面文案「从未应用」。
    applied_at_ago_h: Option<i64>,
    latest_at_ago_h: Option<i64>,
    /// 待应用窗口内的会话（sesno > applied 的那部分）。
    sessions: Vec<SessionPreview>,
    zone_refno: &'static str,
    zone_name: &'static str,
    units: Vec<SimUnit>,
    /// 「SITE 归属未知」桶的净变化（added / modified / deleted）：解析不出 SITE
    /// 祖先的那些改动。全 0 = 不出这一行。
    unknown_site: (u32, u32, u32),
    transform_targets: Vec<TransformTargetPreview>,
    net: (u32, u32, u32, u32, u32), // added / modified / deleted / model_affecting / no_generation
    changed_elements: u64,
    anomaly: Option<FileAnomaly>,
    blocked: bool,
    not_in_project: bool,
    initialization_required: bool,
}

/// 找到（或新建）一个 SITE 桶，返回下标。
fn bucket_of(buckets: &mut Vec<SitePreview>, site: SimSite) -> usize {
    if let Some(i) = buckets.iter().position(|b| b.site_refno == site.refno) {
        return i;
    }
    buckets.push(SitePreview {
        site_refno: site.refno.into(),
        name: site.name.into(),
        ..Default::default()
    });
    buckets.len() - 1
}

fn accumulate(bucket: &mut SitePreview, unit: &UnitPreview) {
    bucket.added += unit.added;
    bucket.modified += unit.modified;
    bucket.deleted += unit.deleted;
    bucket.model_affecting += unit.model_affecting;
    bucket.units.push(unit.clone());
}

impl SimDb {
    fn file_path(&self) -> String {
        if self.file_name.is_empty() {
            String::new()
        } else {
            format!("D:/sim-project/desdb/{}", self.file_name)
        }
    }

    /// 把本库的交付单元按最近 SITE 祖先分桶（ADR-020 `SiteSummary`）。
    ///
    /// 与 gen-model `build_report_rollup` 同口径：挪动的单元在 pre/post 两侧各解
    /// 一次，于是它在两个桶里**各出现一次、计数两边都记**，只有 `moved_out` /
    /// `moved_in` 分左右。所以按桶相加会虚高，去重口径要看 `DbPreview::units`
    /// ——S2-G 的勾选折算正是这么算的。
    fn site_buckets(&self, previews: &[UnitPreview]) -> Vec<SitePreview> {
        let mut buckets: Vec<SitePreview> = Vec::new();
        for (unit, preview) in self.units.iter().zip(previews) {
            // 解不出 SITE 的单元一个桶都不进：整库都是这样时 S2-G 退回文件身份行。
            let Some(site) = unit.site else { continue };
            let post = bucket_of(&mut buckets, site);
            accumulate(&mut buckets[post], preview);
            let Some(from) = unit.moved_from else {
                continue;
            };
            buckets[post].moved_in += 1;
            let pre = bucket_of(&mut buckets, from);
            accumulate(&mut buckets[pre], preview);
            buckets[pre].moved_out += 1;
        }
        buckets.sort_by(|a, b| a.site_refno.cmp(&b.site_refno));
        let (added, modified, deleted) = self.unknown_site;
        if added + modified + deleted > 0 {
            buckets.push(SitePreview {
                added,
                modified,
                deleted,
                ..Default::default()
            });
        }
        buckets
    }

    /// 本期扫描能拿它排批次吗（在项目里、没阻断、还有活）。
    fn runnable(&self) -> bool {
        self.db_type == "DESI"
            && !self.not_in_project
            && !self.blocked
            && (self.latest > self.applied || (self.initialization_required && self.applied == 0))
    }

    /// 已是最新（在范围内、能够到、没活干）。
    fn up_to_date(&self) -> bool {
        self.db_type == "DESI" && !self.not_in_project && !self.blocked && !self.runnable()
    }
}

#[derive(PartialEq)]
enum Stage {
    Queued,
    Applying { until: DateTime<Utc> },
    Generating { unit: usize, until: DateTime<Utc> },
    Finished,
}

struct SimBatch {
    task_id: String,
    dbnum: u32,
    start_sesno: i32,
    end_sesno: i32,
    units: Vec<SimUnit>,
    merged_sesnos: Vec<u32>,
    changed_elements: u64,
    stage: Stage,
    state: &'static str, // 终态串："succeeded" | "partial"
    units_done: u32,
    events_seen: u64,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
}

impl SimBatch {
    fn active(&self) -> bool {
        !matches!(self.stage, Stage::Finished)
    }

    fn running(&self) -> bool {
        matches!(
            self.stage,
            Stage::Applying { .. } | Stage::Generating { .. }
        )
    }
}

struct SimRoom {
    task_id: String,
    done: u32,
    total: u32,
    next_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
}

impl SimRoom {
    /// 目标先收面板后收构件，与真实房间轮的分项口径同形。
    fn counts(&self) -> RoomCounts {
        let left = (self.total - self.done) as usize;
        RoomCounts {
            panels: left.min(2),
            elements: left.saturating_sub(2),
            dead_letters: 0,
        }
    }
}

/// 模拟模型树：两个 SITE，下面挂 ZONE / PIPE / BRAN / EQUI / PANE。
struct SimTree {
    sites: Vec<EleTreeNode>,
    children: HashMap<RefU64, Vec<EleTreeNode>>,
    parents: HashMap<RefU64, RefU64>,
    names: HashMap<String, RefU64>,
    nodes: HashMap<RefU64, EleTreeNode>,
}

impl SimTree {
    fn build() -> Self {
        let mut tree = SimTree {
            sites: Vec::new(),
            children: HashMap::new(),
            parents: HashMap::new(),
            names: HashMap::new(),
            nodes: HashMap::new(),
        };
        let node =
            |id: u64, noun: &str, name: &str, owner: u64, order: u16, kids: u16| EleTreeNode {
                refno: RefU64::from(id).into(),
                noun: noun.into(),
                name: name.into(),
                owner: RefU64::from(owner).into(),
                order,
                children_count: kids,
                ..Default::default()
            };
        let mut register = |n: &EleTreeNode| {
            let refno = n.refno.refno();
            tree.names.insert(n.name.clone(), refno);
            tree.nodes.insert(refno, n.clone());
        };

        let site_a = node(9001, "SITE", "/SIM-SITE-A", 9000, 1, 2);
        let site_b = node(9002, "SITE", "/SIM-SITE-B", 9000, 2, 0);
        let z1 = node(9101, "ZONE", "/SIM-Z1", 9001, 1, 2);
        let z2 = node(9102, "ZONE", "/SIM-Z2", 9001, 2, 2);
        let pipe101 = node(9201, "PIPE", "/SIM-PIPE-101", 9101, 1, 1);
        let eq201 = node(9202, "EQUI", "/SIM-EQ-201", 9101, 2, 0);
        let bran101 = node(9301, "BRAN", "/SIM-PIPE-101/B1", 9201, 1, 0);
        let pane7 = node(9203, "PANE", "/SIM-PANEL-7", 9102, 1, 0);
        let eq305 = node(9204, "EQUI", "/SIM-EQ-305", 9102, 2, 0);

        for n in [
            &site_a, &site_b, &z1, &z2, &pipe101, &eq201, &bran101, &pane7, &eq305,
        ] {
            register(n);
        }
        let link = |tree: &mut SimTree, owner: &EleTreeNode, kids: Vec<EleTreeNode>| {
            let owner_refno = owner.refno.refno();
            for kid in &kids {
                tree.parents.insert(kid.refno.refno(), owner_refno);
            }
            tree.children.insert(owner_refno, kids);
        };
        link(&mut tree, &site_a, vec![z1.clone(), z2.clone()]);
        link(&mut tree, &site_b, Vec::new());
        link(&mut tree, &z1, vec![pipe101.clone(), eq201]);
        link(&mut tree, &z2, vec![pane7, eq305]);
        link(&mut tree, &pipe101, vec![bran101]);
        tree.sites = vec![site_a, site_b];
        tree
    }
}

// ================================================================ 引擎

pub struct Engine {
    /// 对外声称的服务启动时刻。**故意报成十分钟前**：`rebuilt()` 会把
    /// 「启动后两分钟内建的队列行」判成重建残留并挂横幅，全新演示不该顶着它。
    service_started_at: DateTime<Utc>,
    /// 会话写入时刻的锚点（ADR-020 两个时间戳都相对它算）。固定在构造那一刻，
    /// 而不是每次预览取 `now`——否则同一份剧本每查一次时间都在漂。
    epoch: DateTime<Utc>,
    dbs: Vec<SimDb>,
    batches: Vec<SimBatch>,
    room: Option<SimRoom>,
    room_owed: bool,
    paused: bool,
    pending: Vec<PendingModelUnit>,
    /// 已复活、等着自愈消失的死信：(root_refno, 到期时刻)。
    retries_due: Vec<(String, DateTime<Utc>)>,
    /// db7002 排队期间「新到会话并入」的一次性剧本还没演过。
    absorb_pending: bool,
    announced_feed: bool,
    last_activity: DateTime<Utc>,
    seq: u32,
    tree: SimTree,
}

impl Engine {
    pub fn from_env() -> Option<Self> {
        enabled().then(Self::new)
    }

    fn new() -> Self {
        let now = Utc::now();
        Engine {
            service_started_at: now - Duration::minutes(10),
            epoch: now,
            dbs: script(),
            batches: Vec::new(),
            room: None,
            room_owed: false,
            paused: false,
            pending: Vec::new(),
            retries_due: Vec::new(),
            absorb_pending: true,
            announced_feed: false,
            last_activity: now,
            seq: 0,
            tree: SimTree::build(),
        }
    }

    // ------------------------------------------------ 启动序列与模型树

    pub fn ready_info(&self) -> ReadyInfo {
        ReadyInfo {
            project: PROJECT.into(),
            mdb: MDB.into(),
            ns: NAMESPACE.into(),
            db_nums: self
                .dbs
                .iter()
                .filter(|db| db.db_type == "DESI")
                .map(|db| db.dbnum)
                .collect(),
            observed_at: Utc::now(),
            sites: self.tree.sites.clone(),
        }
    }

    pub fn children(&self, refno: RefU64) -> Vec<EleTreeNode> {
        self.tree.children.get(&refno).cloned().unwrap_or_default()
    }

    pub fn props(&self, refno: RefU64) -> Vec<Attr> {
        let attr = |name: &str, value: String, kind: AttrKind| Attr {
            name: name.into(),
            value,
            kind,
        };
        let Some(node) = self.tree.nodes.get(&refno) else {
            return vec![attr("REFNO", refno.to_string(), AttrKind::Opaque)];
        };
        vec![
            attr("NAME", node.name.clone(), AttrKind::Text),
            attr("TYPE", node.noun.clone(), AttrKind::Opaque),
            attr("REFNO", refno.to_string(), AttrKind::Opaque),
            attr("OWNER", node.owner.refno().to_string(), AttrKind::Opaque),
            attr("LOCK", "false".into(), AttrKind::Bool),
            attr(
                "DESC",
                "模拟元素（PLANT_UI_SIM 剧本）".into(),
                AttrKind::Text,
            ),
        ]
    }

    pub fn resolve_name(&self, name: &str) -> Option<RefU64> {
        let name = name.trim();
        self.tree
            .names
            .get(name)
            .or_else(|| {
                self.tree
                    .names
                    .get(&format!("/{}", name.trim_start_matches('/')))
            })
            .copied()
    }

    pub fn ancestors(&self, refno: RefU64) -> Vec<RefU64> {
        let mut chain = vec![refno];
        let mut cur = refno;
        while let Some(owner) = self.tree.parents.get(&cur) {
            chain.push(*owner);
            cur = *owner;
        }
        chain
    }

    pub fn get_work(&self, branches: &[RefU64], reload_models: bool) -> GetWork {
        GetWork {
            sites: self.tree.sites.clone(),
            reload_models,
            branches: branches
                .iter()
                .map(|refno| (*refno, self.children(*refno)))
                .collect(),
            failed: Vec::new(),
        }
    }

    /// 与真查询同口径：DESI、登记过（applied > 0）、文件比水位新。
    pub fn pending_sessions(&self) -> u32 {
        self.dbs
            .iter()
            .filter(|db| db.db_type == "DESI" && db.applied > 0 && db.latest > db.applied)
            .map(|db| (db.latest - db.applied) as u32)
            .sum()
    }

    // ------------------------------------------------ 预览与执行

    /// 一个会话写入时刻的 RFC3339 表示（ADR-020）。
    fn e3d_stamp(&self, ago_h: Option<i64>) -> Option<String> {
        ago_h.map(|h| stamp(self.epoch - Duration::hours(h)))
    }

    pub fn preview(&self) -> Preview {
        let dbnums = self
            .dbs
            .iter()
            .map(|db| {
                let pending_units = db.latest > db.applied && !db.blocked;
                let units: Vec<UnitPreview> = if pending_units {
                    db.units
                        .iter()
                        .enumerate()
                        .map(|(i, unit)| UnitPreview {
                            root_refno: unit.root_refno.into(),
                            noun: unit.noun.into(),
                            name: unit.name.into(),
                            added: u32::from(i == 0),
                            modified: 1 + u32::from(i == 1),
                            deleted: 0,
                            moved_in: u32::from(unit.moved_from.is_some()),
                            moved_out: 0,
                            model_affecting: 2,
                            will_generate: true,
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                let zones = if units.is_empty() {
                    Vec::new()
                } else {
                    vec![ZonePreview {
                        zone_refno: db.zone_refno.into(),
                        name: db.zone_name.into(),
                        added: db.net.0,
                        modified: db.net.1,
                        deleted: db.net.2,
                        model_affecting: db.net.3,
                        units: units.clone(),
                    }]
                };
                DbPreview {
                    dbnum: db.dbnum,
                    db_type: db.db_type.into(),
                    file_name: db.file_name.into(),
                    file_path: db.file_path(),
                    applied_sesno: db.applied,
                    file_latest_sesno: db.latest,
                    applied_sesno_time: self.e3d_stamp(db.applied_at_ago_h),
                    file_latest_sesno_time: self.e3d_stamp(db.latest_at_ago_h),
                    sites: if pending_units {
                        db.site_buckets(&units)
                    } else {
                        Vec::new()
                    },
                    sessions: if pending_units {
                        db.sessions
                            .iter()
                            .filter(|s| s.sesno as i32 > db.applied)
                            .cloned()
                            .collect()
                    } else {
                        Vec::new()
                    },
                    net_added: if pending_units { db.net.0 } else { 0 },
                    net_modified: if pending_units { db.net.1 } else { 0 },
                    net_deleted: if pending_units { db.net.2 } else { 0 },
                    model_affecting: if pending_units { db.net.3 } else { 0 },
                    units,
                    zones,
                    transform_targets: if pending_units {
                        db.transform_targets.clone()
                    } else {
                        Vec::new()
                    },
                    no_generation: if pending_units { db.net.4 } else { 0 },
                    blocked: db.blocked,
                    anomaly: db.anomaly.clone(),
                    initialization_required: db.initialization_required,
                    not_in_project: db.not_in_project,
                }
            })
            .collect();
        Preview {
            project: PROJECT.into(),
            mdb: MDB.into(),
            dbnums,
            pending_model_retries: self.pending.clone(),
            warnings: vec![
                "模拟模式（PLANT_UI_SIM=1）：以下数据由内置剧本生成，仅用于界面演示".into(),
            ],
            up_to_date: !self.dbs.iter().any(SimDb::runnable),
        }
    }

    /// `dbnums` 是勾选集折算的范围内子集（ADR-020 第 3 项）：`None` = 全范围；
    /// `Some` 时未勾选的库不扫描、不入队、水位不动，只进回执的 `unselected`。
    /// **`Some(&[])` 是「一个都不勾」而不是「没选择」**，两者在这里也不许混。
    pub fn execute(&mut self, dbnums: Option<&[u32]>) -> Enqueued {
        let now = Utc::now();
        let mut receipt = Enqueued {
            project: PROJECT.into(),
            ..Default::default()
        };
        // 候选 = 范围内（DESI）且本项目目录里够得着的库；含已最新与被阻断的。
        receipt.scanned = self
            .dbs
            .iter()
            .filter(|db| db.db_type == "DESI" && !db.not_in_project)
            .count();

        let runnable: Vec<u32> = self
            .dbs
            .iter()
            .filter(|db| db.runnable())
            .map(|db| db.dbnum)
            .collect();
        for dbnum in runnable {
            if dbnums.is_some_and(|selected| !selected.contains(&dbnum)) {
                receipt.unselected.push(dbnum);
                continue;
            }
            // 同库已有活动批次：这个静态世界里窗口不会长，按「已被排队行覆盖」计。
            if self.batches.iter().any(|b| b.dbnum == dbnum && b.active()) {
                receipt.already_covered.push(dbnum);
                continue;
            }
            let db = self.dbs.iter().find(|db| db.dbnum == dbnum).unwrap();
            self.seq += 1;
            let task_id = format!("sim-db{}-{}", db.dbnum, self.seq);
            let start_sesno = if db.initialization_required && db.applied == 0 {
                1
            } else {
                db.applied + 1
            };
            let batch = SimBatch {
                task_id: task_id.clone(),
                dbnum: db.dbnum,
                start_sesno,
                end_sesno: db.latest,
                units: db.units.clone(),
                merged_sesnos: Vec::new(),
                changed_elements: db.changed_elements,
                stage: Stage::Queued,
                state: "succeeded",
                units_done: 0,
                events_seen: 0,
                created_at: now,
                started_at: None,
                finished_at: None,
            };
            let position = self
                .batches
                .iter()
                .filter(|b| matches!(b.stage, Stage::Queued))
                .count()
                + 1;
            receipt.enqueued.push(EnqueuedBatch {
                task_id,
                dbnum: db.dbnum,
                db_type: "DESI".into(),
                position,
                start_sesno,
                end_sesno: db.latest,
            });
            self.batches.push(batch);
        }
        for db in &self.dbs {
            if db.db_type != "DESI" || db.not_in_project {
                continue;
            }
            if db.blocked {
                receipt.blocked.push(BlockedDbnum {
                    dbnum: db.dbnum,
                    reason: db
                        .anomaly
                        .as_ref()
                        .map(|a| a.to_string())
                        .unwrap_or_else(|| "文件异常".into()),
                });
            } else if db.up_to_date()
                && !receipt.already_covered.contains(&db.dbnum)
                && !self
                    .batches
                    .iter()
                    .any(|b| b.dbnum == db.dbnum && b.active())
            {
                receipt.up_to_date += 1;
            }
        }
        self.last_activity = now;
        receipt
    }

    // ------------------------------------------------ 队列面板

    pub fn poll(&self) -> Poll {
        let now = Utc::now();
        let rows = self
            .batches
            .iter()
            .filter(|b| b.active())
            .map(|b| QueueRow {
                task_id: b.task_id.clone(),
                dbnum: b.dbnum,
                db_type: "DESI".into(),
                state: if b.running() { "running" } else { "queued" }.into(),
                start_sesno: b.start_sesno,
                end_sesno: b.end_sesno,
            })
            .collect();

        let mut tasks: Vec<TaskEntry> = self.batches.iter().map(|b| self.batch_entry(b)).collect();
        if let Some(room) = &self.room {
            tasks.push(self.room_entry(room));
        }
        tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Poll {
            queue: QueueSnapshot {
                paused: self.paused,
                rows,
            },
            tasks,
            tasks_error: None,
            health: Some(Health {
                project: PROJECT.into(),
                mdb: Some(MDB.into()),
                namespace: Some(NAMESPACE.into()),
                sync_live: true,
                started_at: stamp(self.service_started_at),
                gen_spatial_tree: true,
                queue_paused: self.paused,
                worker_alive: Some(true),
                worker_idle_secs: Some((now - self.last_activity).num_seconds().max(0) as u64),
            }),
            pending: self.pending.clone(),
            pending_known: true,
            dbnums: self
                .dbs
                .iter()
                .map(|db| DbnumStatus {
                    dbnum: db.dbnum,
                    db_type: db.db_type.into(),
                    anomaly: db.anomaly.clone(),
                    blocked: db.blocked,
                    excluded: db.db_type != "DESI",
                    not_in_project: db.not_in_project,
                })
                .collect(),
        }
    }

    fn batch_entry(&self, batch: &SimBatch) -> TaskEntry {
        let state = match batch.stage {
            Stage::Queued => "queued",
            Stage::Applying { .. } | Stage::Generating { .. } => "running",
            Stage::Finished => batch.state,
        };
        // 「应用中 / 生成中」的轮询侧分界：worker 交给单元循环那一刻才写 total_units。
        let generating = matches!(batch.stage, Stage::Generating { .. } | Stage::Finished);
        let result = matches!(batch.stage, Stage::Finished).then(|| Outcome {
            project: PROJECT.into(),
            status: if batch.state == "succeeded" {
                RunStatus::Success
            } else {
                RunStatus::Partial
            },
            absorbed: false,
            batch: Some(BatchResult {
                dbnum: batch.dbnum,
                db_type: "DESI".into(),
                file_path: self
                    .dbs
                    .iter()
                    .find(|db| db.dbnum == batch.dbnum)
                    .map(SimDb::file_path)
                    .unwrap_or_default(),
                start_sesno: batch.start_sesno,
                end_sesno: batch.end_sesno,
                status: BatchStatus::Applied,
                message: None,
                merged_sesnos: batch.merged_sesnos.clone(),
                changed_elements: batch.changed_elements,
            }),
            units: batch
                .units
                .iter()
                .map(|unit| UnitResult {
                    dbnum: batch.dbnum,
                    root_refno: unit.root_refno.into(),
                    noun: unit.noun.into(),
                    status: if unit.fail.is_some() {
                        UnitStatus::Failed
                    } else {
                        UnitStatus::Generated
                    },
                    attempts: if unit.fail.is_some() { 3 } else { 1 },
                    message: unit.fail.map(Into::into),
                    old_owner: None,
                    new_owner: None,
                })
                .collect(),
            warnings: Vec::new(),
        });
        TaskEntry {
            task_id: batch.task_id.clone(),
            kind: KIND_DATA_BATCH.into(),
            state: state.into(),
            project: PROJECT.into(),
            created_at: stamp(batch.created_at),
            started_at: batch.started_at.map(stamp),
            finished_at: batch.finished_at.map(stamp),
            dbnum: Some(batch.dbnum),
            db_type: Some("DESI".into()),
            start_sesno: Some(batch.start_sesno),
            end_sesno: Some(batch.end_sesno),
            units_done: generating.then_some(batch.units_done),
            total_units: generating.then_some(batch.units.len() as u32),
            events_seen: batch.events_seen,
            detail: None,
            result,
        }
    }

    fn room_entry(&self, room: &SimRoom) -> TaskEntry {
        TaskEntry {
            task_id: room.task_id.clone(),
            kind: KIND_ROOM_RECALC.into(),
            state: if room.finished_at.is_some() {
                "succeeded"
            } else {
                "running"
            }
            .into(),
            project: PROJECT.into(),
            created_at: stamp(room.created_at),
            started_at: Some(stamp(room.created_at)),
            finished_at: room.finished_at.map(stamp),
            dbnum: None,
            db_type: None,
            start_sesno: None,
            end_sesno: None,
            units_done: Some(room.done),
            total_units: Some(room.total),
            events_seen: 0,
            detail: Some(room.counts()),
            result: None,
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    pub fn retry_pending_unit(&mut self, root_refno: &str) -> anyhow::Result<()> {
        let Some(unit) = self
            .pending
            .iter_mut()
            .find(|unit| unit.root_refno == root_refno)
        else {
            anyhow::bail!("待重试行不存在，可能刚被复活并跑掉了（模拟 404）");
        };
        unit.attempts = 0;
        unit.dead = false;
        self.retries_due.push((
            root_refno.to_owned(),
            Utc::now() + Duration::milliseconds(RETRY_MS),
        ));
        Ok(())
    }

    // ------------------------------------------------ 推演

    /// 每帧推演一步，返回要经 `evt_tx` 发出的事件（WS 同形明细 + 醒钟）。
    pub fn pump(&mut self) -> Vec<Evt> {
        let now = Utc::now();
        let mut out = Vec::new();

        if !self.announced_feed {
            self.announced_feed = true;
            out.push(Evt::QueueFeedLive);
        }

        // 复活的死信到点自愈：从持久表消失，下一拍轮询看得见。
        let mut healed = false;
        self.retries_due.retain(|(root, due)| {
            if now < *due {
                return true;
            }
            self.pending.retain(|unit| unit.root_refno != *root);
            healed = true;
            false
        });
        if healed {
            self.last_activity = now;
            out.push(Evt::QueueTaskChanged);
        }

        self.step_worker(now, &mut out);
        self.step_room(now, &mut out);
        out
    }

    fn step_worker(&mut self, now: DateTime<Utc>, out: &mut Vec<Evt>) {
        // 推进正在跑的批次。
        if let Some(idx) = self.batches.iter().position(SimBatch::running) {
            let batch = &mut self.batches[idx];
            match batch.stage {
                Stage::Applying { until } if now >= until => {
                    if batch.units.is_empty() {
                        Self::finish_batch(batch, now, out);
                        self.after_batch_finished(idx, now);
                    } else {
                        batch.stage = Stage::Generating {
                            unit: 0,
                            until: now + Duration::milliseconds(UNIT_MS),
                        };
                        let unit = &batch.units[0];
                        batch.events_seen += 1;
                        out.push(Evt::QueueProgress(
                            batch.task_id.clone(),
                            ProgressEvent::ModelUnitStarted {
                                dbnum: batch.dbnum,
                                root_refno: unit.root_refno.into(),
                                noun: unit.noun.into(),
                            },
                        ));
                        // db7001 进入生成段的同时演一次「新会话并入排队行」：
                        // db7002 的排队窗口右端被推高，结果摘要的 merged_sesnos 有据可查。
                        if self.absorb_pending {
                            self.absorb_pending = false;
                            self.absorb_new_session(out);
                        }
                    }
                }
                Stage::Generating { unit, until } if now >= until => {
                    let (done_msg, task_id, dbnum);
                    {
                        let batch = &mut self.batches[idx];
                        let sim_unit = batch.units[unit].clone();
                        batch.units_done += 1;
                        batch.events_seen += 1;
                        task_id = batch.task_id.clone();
                        dbnum = batch.dbnum;
                        done_msg = Evt::QueueProgress(
                            task_id.clone(),
                            ProgressEvent::ModelUnitFinished {
                                dbnum,
                                root_refno: sim_unit.root_refno.into(),
                                success: sim_unit.fail.is_none(),
                                message: sim_unit.fail.map(Into::into),
                            },
                        );
                        if let Some(error) = sim_unit.fail {
                            batch.state = "partial";
                            self.pending.push(PendingModelUnit {
                                dbnum,
                                root_refno: sim_unit.root_refno.into(),
                                noun: sim_unit.noun.into(),
                                source_end_sesno: batch.end_sesno,
                                attempts: 3,
                                last_error: Some(error.into()),
                                dead: true,
                            });
                        }
                    }
                    out.push(done_msg);
                    let batch = &mut self.batches[idx];
                    if unit + 1 < batch.units.len() {
                        let next = &batch.units[unit + 1];
                        batch.events_seen += 1;
                        out.push(Evt::QueueProgress(
                            batch.task_id.clone(),
                            ProgressEvent::ModelUnitStarted {
                                dbnum: batch.dbnum,
                                root_refno: next.root_refno.into(),
                                noun: next.noun.into(),
                            },
                        ));
                        batch.stage = Stage::Generating {
                            unit: unit + 1,
                            until: now + Duration::milliseconds(UNIT_MS),
                        };
                    } else {
                        Self::finish_batch(batch, now, out);
                        self.after_batch_finished(idx, now);
                    }
                }
                _ => {}
            }
            return;
        }

        // 没有运行行：暂停只挡出队，这里就是那道闸。
        if self.paused {
            return;
        }
        if let Some(batch) = self
            .batches
            .iter_mut()
            .filter(|b| matches!(b.stage, Stage::Queued))
            .min_by_key(|b| b.created_at)
        {
            batch.stage = Stage::Applying {
                until: now + Duration::milliseconds(APPLY_MS),
            };
            batch.started_at = Some(now);
            batch.events_seen += 1;
            out.push(Evt::QueueProgress(
                batch.task_id.clone(),
                ProgressEvent::DataBatchStarted {
                    dbnum: batch.dbnum,
                    start_sesno: batch.start_sesno,
                    end_sesno: batch.end_sesno,
                },
            ));
            out.push(Evt::QueueTaskChanged);
            self.last_activity = now;
        }
    }

    fn finish_batch(batch: &mut SimBatch, now: DateTime<Utc>, out: &mut Vec<Evt>) {
        batch.stage = Stage::Finished;
        batch.finished_at = Some(now);
        batch.events_seen += 1;
        out.push(Evt::QueueProgress(
            batch.task_id.clone(),
            ProgressEvent::DataBatchFinished {
                dbnum: batch.dbnum,
                success: true,
                message: None,
            },
        ));
        out.push(Evt::QueueTaskChanged);
    }

    /// 批次终态后的世界推进：水位前移、需初始化落位、房间轮欠账。
    fn after_batch_finished(&mut self, idx: usize, now: DateTime<Utc>) {
        let (dbnum, end, had_units) = {
            let b = &self.batches[idx];
            (b.dbnum, b.end_sesno, !b.units.is_empty())
        };
        if let Some(db) = self.dbs.iter_mut().find(|db| db.dbnum == dbnum) {
            db.applied = end;
            db.latest = db.latest.max(end);
            // 水位追平文件之后，「上次应用」就是最新那个会话的写入时刻。
            db.applied_at_ago_h = db.latest_at_ago_h;
            db.initialization_required = false;
        }
        if had_units {
            self.room_owed = true;
        }
        self.last_activity = now;
    }

    /// 一次性剧本：db7001 开始生成时，db7002 的排队行并入新到的会话 57。
    fn absorb_new_session(&mut self, out: &mut Vec<Evt>) {
        let Some(batch) = self
            .batches
            .iter_mut()
            .find(|b| b.dbnum == 7002 && matches!(b.stage, Stage::Queued))
        else {
            return;
        };
        batch.end_sesno += 1;
        batch.merged_sesnos.push(batch.end_sesno as u32);
        if let Some(db) = self.dbs.iter_mut().find(|db| db.dbnum == 7002) {
            db.latest = batch.end_sesno;
            // 刚刚才写进文件的会话：时间戳就是此刻。
            db.latest_at_ago_h = Some(0);
            db.sessions.push(SessionPreview {
                sesno: batch.end_sesno as u32,
                added: 0,
                modified: 1,
                deleted: 0,
            });
        }
        out.push(Evt::QueueTaskChanged);
    }

    fn step_room(&mut self, now: DateTime<Utc>, out: &mut Vec<Evt>) {
        if let Some(room) = &mut self.room {
            if room.finished_at.is_none() {
                while now >= room.next_at && room.done < room.total {
                    room.done += 1;
                    room.next_at += Duration::milliseconds(ROOM_STEP_MS);
                }
                if room.done >= room.total {
                    room.finished_at = Some(now);
                    self.last_activity = now;
                    out.push(Evt::QueueTaskChanged);
                }
                return;
            }
        }
        // 队列收空、且这一轮欠着房间账 → 开一轮房间归属重算。
        if self.room_owed && !self.batches.iter().any(SimBatch::active) {
            self.room_owed = false;
            self.seq += 1;
            self.room = Some(SimRoom {
                task_id: format!("sim-room-{}", self.seq),
                done: 0,
                total: 16,
                next_at: now + Duration::milliseconds(ROOM_STEP_MS),
                created_at: now,
                finished_at: None,
            });
            self.last_activity = now;
            out.push(Evt::QueueTaskChanged);
        }
    }
}

/// 内置剧本的初始世界。
fn script() -> Vec<SimDb> {
    let session = |sesno: u32, added: u32, modified: u32, deleted: u32| SessionPreview {
        sesno,
        added,
        modified,
        deleted,
    };
    vec![
        // 两个单元分处两个 SITE：S2-G 顶层出两行、勾任一行同库联动，展开的详情行
        // 里「与 … 同库联动」那句才有东西可说。
        SimDb {
            dbnum: 7001,
            db_type: "DESI",
            file_name: "des001.db",
            applied: 41,
            latest: 43,
            applied_at_ago_h: Some(72),
            latest_at_ago_h: Some(4),
            sessions: vec![session(42, 3, 2, 0), session(43, 0, 1, 1)],
            zone_refno: "9101/1",
            zone_name: "/SIM-Z1",
            units: vec![
                SimUnit {
                    root_refno: "24384/101",
                    noun: "BRAN",
                    name: "/SIM-PIPE-101/B1",
                    site: Some(SITE_A),
                    moved_from: None,
                    fail: None,
                },
                SimUnit {
                    root_refno: "24384/201",
                    noun: "EQUI",
                    name: "/SIM-EQ-201",
                    site: Some(SITE_B),
                    moved_from: None,
                    fail: None,
                },
            ],
            unknown_site: (0, 0, 0),
            transform_targets: vec![TransformTargetPreview {
                refno: "24384/301".into(),
                noun: "PIPE".into(),
                name: "/SIM-PIPE-103".into(),
                container: true,
            }],
            net: (3, 3, 1, 4, 1),
            changed_elements: 7,
            anomaly: None,
            blocked: false,
            not_in_project: false,
            initialization_required: false,
        },
        // 跨 SITE 挪动 + 归属未知桶都在这一库：`/SIM-EQ-305` 从 A 挪到 B，两个桶里
        // 各出现一次（A 记 moved_out、B 记 moved_in）；`unknown_site` 那一笔就是
        // `no_generation` 那个解不出 SITE 祖先的改动。
        SimDb {
            dbnum: 7002,
            db_type: "DESI",
            file_name: "des002.db",
            applied: 55,
            latest: 56,
            applied_at_ago_h: Some(192),
            latest_at_ago_h: Some(26),
            sessions: vec![session(56, 0, 4, 0)],
            zone_refno: "9102/1",
            zone_name: "/SIM-Z2",
            units: vec![
                SimUnit {
                    root_refno: "24385/11",
                    noun: "BRAN",
                    name: "/SIM-PIPE-102/B1",
                    site: Some(SITE_A),
                    moved_from: None,
                    fail: None,
                },
                SimUnit {
                    root_refno: "24385/77",
                    noun: "PANE",
                    name: "/SIM-PANEL-7",
                    site: Some(SITE_A),
                    moved_from: None,
                    fail: Some("OCC 布尔运算失败：负体面片退化（剧本注入的失败）"),
                },
                SimUnit {
                    root_refno: "24385/305",
                    noun: "EQUI",
                    name: "/SIM-EQ-305",
                    site: Some(SITE_B),
                    moved_from: Some(SITE_A),
                    fail: None,
                },
            ],
            unknown_site: (0, 1, 0),
            transform_targets: Vec::new(),
            net: (0, 4, 0, 3, 1),
            changed_elements: 4,
            anomaly: None,
            blocked: false,
            not_in_project: false,
            initialization_required: false,
        },
        SimDb {
            dbnum: 7003,
            db_type: "DESI",
            file_name: "des003.db",
            applied: 88,
            latest: 88,
            applied_at_ago_h: Some(48),
            latest_at_ago_h: Some(48),
            sessions: Vec::new(),
            zone_refno: "",
            zone_name: "",
            units: Vec::new(),
            unknown_site: (0, 0, 0),
            transform_targets: Vec::new(),
            net: (0, 0, 0, 0, 0),
            changed_elements: 0,
            anomaly: None,
            blocked: false,
            not_in_project: false,
            initialization_required: false,
        },
        SimDb {
            dbnum: 7004,
            db_type: "DESI",
            file_name: "des004.db",
            applied: 70,
            latest: 66,
            // 回退的库两个时间也是倒着的：文件最新那个会话比已应用的还老。
            applied_at_ago_h: Some(120),
            latest_at_ago_h: Some(288),
            sessions: Vec::new(),
            zone_refno: "",
            zone_name: "",
            units: Vec::new(),
            unknown_site: (0, 0, 0),
            transform_targets: Vec::new(),
            net: (0, 0, 0, 0, 0),
            changed_elements: 0,
            anomaly: Some(FileAnomaly::Rollback {
                file_latest_sesno: 66,
                applied_sesno: 70,
            }),
            blocked: true,
            not_in_project: false,
            initialization_required: false,
        },
        // MDB 声明、当前项目目录里没有它的文件：本期够不着，不阻断也不执行。
        SimDb {
            dbnum: 7005,
            db_type: "DESI",
            file_name: "",
            applied: 0,
            latest: 0,
            // 够不着的库连文件都没有，读不到任何会话页。
            applied_at_ago_h: None,
            latest_at_ago_h: None,
            sessions: Vec::new(),
            zone_refno: "",
            zone_name: "",
            units: Vec::new(),
            unknown_site: (0, 0, 0),
            transform_targets: Vec::new(),
            net: (0, 0, 0, 0, 0),
            changed_elements: 0,
            anomaly: None,
            blocked: false,
            not_in_project: true,
            initialization_required: true,
        },
        // 首次导入：契约不为它解会话区间，net 全 0，也没有 pre/post 窗口可分 SITE
        // 桶（`site: None`），但它确实会跑。S2-G 上它退回文件身份行——优雅降级
        // 那条分支的现成样本，别顺手给它编个 SITE。
        SimDb {
            dbnum: 7006,
            db_type: "DESI",
            file_name: "des006.db",
            applied: 0,
            latest: 12,
            // 从未应用：界面文案「从未应用 → 文件最新 …」。
            applied_at_ago_h: None,
            latest_at_ago_h: Some(6),
            sessions: Vec::new(),
            zone_refno: "",
            zone_name: "",
            units: vec![
                SimUnit {
                    root_refno: "24390/1",
                    noun: "EQUI",
                    name: "/SIM-NEW-EQ-1",
                    site: None,
                    moved_from: None,
                    fail: None,
                },
                SimUnit {
                    root_refno: "24390/2",
                    noun: "BRAN",
                    name: "/SIM-NEW-PIPE-1/B1",
                    site: None,
                    moved_from: None,
                    fail: None,
                },
            ],
            unknown_site: (0, 0, 0),
            transform_targets: Vec::new(),
            net: (0, 0, 0, 0, 0),
            changed_elements: 260,
            anomaly: None,
            blocked: false,
            not_in_project: false,
            initialization_required: true,
        },
        SimDb {
            dbnum: 8001,
            db_type: "CATA",
            file_name: "cata001.db",
            applied: 30,
            latest: 30,
            applied_at_ago_h: Some(720),
            latest_at_ago_h: Some(720),
            sessions: Vec::new(),
            zone_refno: "",
            zone_name: "",
            units: Vec::new(),
            unknown_site: (0, 0, 0),
            transform_targets: Vec::new(),
            net: (0, 0, 0, 0, 0),
            changed_elements: 0,
            anomaly: None,
            blocked: false,
            not_in_project: false,
            initialization_required: false,
        },
    ]
}

// ================================================================ 请求分发

/// sim 模式下数据线程的请求处理：与真实分支一一对应，回的都是同一批 `Evt`。
///
/// `Models` / `ModelScopes` 不会到这里——它们在桥上就被送去专用模型通道，
/// 模型通道自己按 [`enabled`] 短路成空结果。
pub fn handle(engine: &mut Engine, req: crate::data::Req, evt_tx: &std::sync::mpsc::Sender<Evt>) {
    use crate::data::Req;
    let evt = match req {
        Req::Reconnect => Evt::Ready(Ok(engine.ready_info())),
        Req::Children(refno) => Evt::Children(refno, Ok(engine.children(refno))),
        Req::Props(refno) => Evt::Props(refno, Ok(engine.props(refno))),
        // 剧本里没有房间数据：空归属即「无所属房间」，界面照常走 Ready 空态。
        Req::ElementRooms(refno) => Evt::ElementRooms(refno, Ok(Vec::new())),
        // 同理，任何房间在剧本里都查不到。
        Req::RoomDetail(refno) => Evt::RoomDetail(refno, Ok(None)),
        // 剧本里没有房间：浏览器打开就是一张空表。
        Req::RoomsOverview => Evt::RoomsOverview(Ok(Vec::new())),
        Req::ResolveName(name) => {
            let result = engine.resolve_name(&name);
            Evt::ResolvedName(name, Ok(result))
        }
        Req::Ancestors(refno) => Evt::Ancestors(refno, Ok(engine.ancestors(refno))),
        Req::ModelUpdatePreview { .. } => Evt::ModelUpdatePreview(Ok(engine.preview())),
        Req::ModelUpdateExecute {
            dbnums,
            from_wizard,
            ..
        } => Evt::ModelUpdateExecute {
            from_wizard,
            result: Ok(engine.execute(dbnums.as_deref())),
        },
        Req::GetWork {
            branches,
            reload_models,
        } => Evt::GetWork(Ok(engine.get_work(&branches, reload_models))),
        Req::PendingSessions => Evt::PendingSessions(Ok(engine.pending_sessions())),
        Req::QueuePoll { .. } => Evt::QueuePoll(Ok(engine.poll())),
        Req::QueueSetPaused { paused, .. } => {
            engine.set_paused(paused);
            Evt::QueueSetPaused(paused, Ok(()))
        }
        Req::RetryPendingUnit { root_refno, .. } => {
            let result = engine.retry_pending_unit(&root_refno);
            Evt::RetryPendingUnit(root_refno, result)
        }
        Req::DataPublish { request, .. } => {
            Evt::DataPublish(request, Ok("模拟模式：已受理（未真正提交）".into()))
        }
        Req::Models(..) | Req::ModelScopes { .. } => {
            unreachable!("模型请求已在桥上路由到专用通道")
        }
    };
    let _ = evt_tx.send(evt);
}
