//! 模型增量更新任务窗：更新预览 → 确认执行（S2 / S2-B 两步向导）。
//!
//! 第三步「执行进度」随 ADR-0011 整体迁去常驻的任务队列视图：确认之后是「扫描 +
//! 入队」，回执进日志、焦点跳到队列面板、本窗关闭——向导不再跟着任何一次运行走。
//! 其余画板都是这两步的形态：S2-A 是第一步的等待态、S2-D 收拢不可用与失败态、
//! S2-E 收拢七种设计库行状态。
//!
//! 窗口自绘外壳（窗口头 52 / 步骤条 48 / 主体 / 底部操作 56）。不用 egui 的默认标题栏：
//! 步骤条要贴在标题底下，默认标题栏塞不进去，两条横栏也对不上一套配色。
//!
//! **界面上每个数字都要指得到契约字段或本地算得出的量，指不到就不摆。**
//! 逐项出处见 `design/MODEL-UPDATE-FIELD-MAP.md`。分组一律按 dbnum——它既是执行边界，
//! 也是任务队列的键（`docs/adr/0011`）。

use std::collections::HashSet;
use std::str::FromStr;
use web_time::{Duration, Instant};

use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Layout, Margin, Rect, RichText, ScrollArea,
    Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};
use egui_phosphor::regular as ph;
use serde::Deserialize;

use crate::style::group_number;
use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Status, Tokens, radius};
use crate::style::widgets;
use crate::{Cmd, RefU64};

// ================================================================ 预览契约

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Preview {
    pub project: String,
    /// 本期执行范围是照哪个 MDB 解的（服务端回显，带前导 `/`）。
    ///
    /// 范围由 MDB 定，界面就得说得出自己看的是哪个 MDB 的范围。服务端与客户端
    /// 各有一份 `mdb_name` 配置，不回显的话两边错开是静默的。
    #[serde(default)]
    pub mdb: String,
    pub dbnums: Vec<DbPreview>,
    #[serde(default)]
    pub pending_model_retries: Vec<PendingModelUnit>,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub up_to_date: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DbPreview {
    pub dbnum: u32,
    pub db_type: String,
    pub file_name: String,
    pub file_path: String,
    pub applied_sesno: i32,
    pub file_latest_sesno: i32,
    /// `applied_sesno` 那个会话在 **E3D 里被写入的时刻**（RFC3339，ADR-020）。
    /// 从未应用或会话页读不到 → `None`，界面文案「从未应用」。老服务端没有
    /// 这个字段——缺席时详情行不摆时间对，不摆假数据。
    #[serde(default)]
    pub applied_sesno_time: Option<String>,
    /// `file_latest_sesno` 那个会话的 E3D 写入时刻。与 `applied_sesno_time`
    /// 同一把尺子，相减 = 文件里还有多大时间跨度的设计改动没被吸收。
    #[serde(default)]
    pub file_latest_sesno_time: Option<String>,
    pub sessions: Vec<SessionPreview>,
    pub net_added: u32,
    pub net_modified: u32,
    pub net_deleted: u32,
    pub model_affecting: u32,
    pub units: Vec<UnitPreview>,
    #[serde(default)]
    pub zones: Vec<ZonePreview>,
    /// 净变化按最近 SITE 祖先分桶（ADR-020 `SiteSummary`）。S2-G 预览树的顶层
    /// 语言；执行边界仍是（dbnum, 会话号区间），SITE 只是选取入口与报告口径。
    /// 老服务端没有这个字段——缺席时该库退回文件身份行（优雅降级，不装死）。
    #[serde(default)]
    pub sites: Vec<SitePreview>,
    /// 纯位姿变更目标（与 `units` 互斥的另一类工作：变换刷新，不重建网格）。
    #[serde(default)]
    pub transform_targets: Vec<TransformTargetPreview>,
    pub no_generation: u32,
    pub blocked: bool,
    pub anomaly: Option<FileAnomaly>,
    #[serde(default)]
    pub initialization_required: bool,
    /// 当前 MDB 声明了这个库，但当前项目目录里没有它的文件。
    ///
    /// 与「文件缺失」隔开：那是登记过、文件后来找不到了，阻断，等人补文件或注销
    /// 登记；这一条是从没登记过，文件多半在别的项目目录里——`/ALL` 声明的 29 个
    /// DESI 里有 9 个是 AvevaCatalogue 的模板与标准库，而扫描按契约只走当前项目。
    /// 它不阻断也不执行，摆出来只为回答「MDB 说 29 个，范围表怎么只有 20 个」。
    #[serde(default)]
    pub not_in_project: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionPreview {
    pub sesno: u32,
    pub added: u32,
    pub modified: u32,
    pub deleted: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ZonePreview {
    pub zone_refno: String,
    pub name: String,
    #[serde(default)]
    pub added: u32,
    #[serde(default)]
    pub modified: u32,
    #[serde(default)]
    pub deleted: u32,
    #[serde(default)]
    pub model_affecting: u32,
    pub units: Vec<UnitPreview>,
}

impl ZonePreview {
    /// `zone_refno` 为空的那桶就是「归属未知」，调用点先判空再取名。
    pub fn name_or_refno(&self) -> &str {
        if self.name.is_empty() {
            &self.zone_refno
        } else {
            &self.name
        }
    }
}

/// ADR-020 `SiteSummary`：一个库的净变化按最近 SITE 祖先分出的一桶。
/// 挪动的交付单元可以出现在两个桶里（pre/post 两侧各解一次）。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SitePreview {
    /// 空串 = 「SITE 归属未知」桶（解析不出 SITE 祖先的变化）。
    pub site_refno: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub added: u32,
    #[serde(default)]
    pub modified: u32,
    #[serde(default)]
    pub deleted: u32,
    #[serde(default)]
    pub moved_in: u32,
    #[serde(default)]
    pub moved_out: u32,
    #[serde(default)]
    pub model_affecting: u32,
    #[serde(default)]
    pub units: Vec<UnitPreview>,
}

impl SitePreview {
    /// 归属未知桶：行形态同 SITE 行、无勾选框、排在同库 SITE 行之后（§2-G）。
    pub fn unknown(&self) -> bool {
        self.site_refno.is_empty()
    }

    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            &self.site_refno
        } else {
            &self.name
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnitPreview {
    pub root_refno: String,
    pub noun: String,
    pub name: String,
    #[serde(default)]
    pub added: u32,
    #[serde(default)]
    pub modified: u32,
    #[serde(default)]
    pub deleted: u32,
    /// 元素跨交付单元迁入 / 迁出的计数。最容易让人困惑的一类变化，单独说。
    #[serde(default)]
    pub moved_in: u32,
    #[serde(default)]
    pub moved_out: u32,
    #[serde(default)]
    pub model_affecting: u32,
    /// 执行时会不会重新生成它。契约默认给 true 的那批老 payload 里没有这个字段，
    /// 缺省当作会生成——少标一行「不生成」比凭空标一行「会生成」安全。
    #[serde(default = "yes")]
    pub will_generate: bool,
}

fn yes() -> bool {
    true
}

/// 一个纯位姿（`POS`/`ORI`）变更目标：执行阶段走 `transform` 便宜工作项——
/// 只刷新世界变换、包围盒、空间树与房间归属，不重建网格、不整单重生成，
/// 所以它不出现在交付单元列表里。老服务端的 payload 没有这个字段，缺省为空。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TransformTargetPreview {
    /// PDMS `a/b` 引用串。
    pub refno: String,
    #[serde(default)]
    pub noun: String,
    #[serde(default)]
    pub name: String,
    /// 粗层级容器（WORL/SITE/ZONE）：执行时会刷新其**整棵子树**的模型实例。
    #[serde(default)]
    pub container: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PendingModelUnit {
    pub dbnum: u32,
    pub root_refno: String,
    #[serde(default)]
    pub noun: String,
    pub source_end_sesno: i32,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub last_error: Option<String>,
    /// 已达服务端的重试上限：自动路径**永不再碰**它，只有人按一下才会动。
    ///
    /// 判定必须由服务端给——上限是它那边的常量，本端拿着 `attempts` 判不出来。
    /// 缺省 false 是有意的：老服务端不带这个字段，那时候少标一行「已放弃」，
    /// 比凭空标一行安全。
    #[serde(default)]
    pub dead: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileAnomaly {
    Rollback {
        file_latest_sesno: i32,
        applied_sesno: i32,
    },
    PathMigrated {
        old_path: String,
        new_path: String,
    },
    TypeChanged {
        stored_db_type: String,
        observed_db_type: String,
    },
    Duplicate {
        paths: Vec<String>,
    },
    Missing {
        path: String,
    },
}

impl std::fmt::Display for FileAnomaly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rollback {
                file_latest_sesno,
                applied_sesno,
            } => write!(
                f,
                "文件最新会话号 {file_latest_sesno} 低于已应用 {applied_sesno}。水位只前进不后退，其他批次不受影响。出路：换回正确文件。"
            ),
            Self::PathMigrated { old_path, new_path } => write!(
                f,
                "同号同类型、水位没变，只是文件被移动了：{old_path} → {new_path}。登记路径可以自动跟新。"
            ),
            Self::TypeChanged {
                stored_db_type,
                observed_db_type,
            } => write!(
                f,
                "同一个 dbnum 这次读到的是 {observed_db_type}，登记的是 {stored_db_type}。身份永不自动覆盖，必须人工确认到底哪个对。"
            ),
            Self::Duplicate { paths } => write!(
                f,
                "项目内发现 {} 个同号文件，不自动挑选；出路：人工挑一个。\n· {}",
                paths.len(),
                paths.join("\n· ")
            ),
            Self::Missing { path } => write!(
                f,
                "已登记的路径上找不到文件了：{path}。出路二选一：把文件放回去，或注销这条登记。"
            ),
        }
    }
}

/// S2-E 的七种设计库行形态，外加两种「不在范围内」与「无变化」。
///
/// `blocked` 不是随 `anomaly` 一起来的，是算出来的：`preview_dbnum` 里只把
/// `Rollback | TypeChanged` 判为阻断，项目扫描器再把 `Duplicate` / `Missing`
/// 直接置 `blocked = true`。**五种异常里只有 `PathMigrated` 照常执行。**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbForm {
    /// 非 DESI：压根不进本期扫描，与「阻断」来自契约的不同位置，不许合成一行。
    Excluded,
    /// 当前 MDB 声明了它，但当前项目目录里没有它的文件——多半在别的项目里
    /// （AvevaCatalogue 的模板与标准库就是这样）。它与「文件缺失」的分别是
    /// **有没有人登记过**：缺失是登记过的文件不见了，得阻断；这个从没登记过，
    /// 本期扫描只是够不着它。
    NotInProject,
    /// 从未导入过，还没有权威水位。契约不为它解会话区间，`net_*` 全是 0，
    /// **但它确实会跑**，绝不能显示成「无变化」。
    Initialize,
    PathMigrated,
    Pending,
    UpToDate,
    Rollback,
    TypeChanged,
    Duplicate,
    Missing,
}

impl DbForm {
    pub fn blocks(self) -> bool {
        matches!(
            self,
            Self::Rollback | Self::TypeChanged | Self::Duplicate | Self::Missing
        )
    }

    /// 行上那枚状态词。空串表示这一形态不值得单独标一句。
    pub fn label(self) -> &'static str {
        match self {
            Self::Excluded => "",
            Self::NotInProject => "MDB 声明",
            Self::Initialize => "需初始化",
            Self::PathMigrated => "路径迁移",
            Self::Pending => "正常待更新",
            Self::UpToDate => "",
            Self::Rollback => "文件回退",
            Self::TypeChanged => "类型变化",
            Self::Duplicate => "同号重复",
            Self::Missing => "文件缺失",
        }
    }

    /// 行尾那句「会不会执行」。它与 label 分两处摆：一句说这库怎么了，
    /// 一句说这次拿它怎么办。
    pub fn note(self) -> &'static str {
        match self {
            Self::Excluded => "非 DESI · 不在本期执行范围",
            Self::NotInProject => "不在当前项目目录 · 本期够不着",
            Self::Initialize => "首次导入 · 会执行",
            Self::PathMigrated => "登记路径自动跟新 · 会执行",
            Self::Pending => "",
            Self::UpToDate => "无变化",
            _ => "已阻断 · 水位不变",
        }
    }

    pub fn tone(self) -> Status {
        match self {
            Self::Excluded | Self::NotInProject | Self::UpToDate => Status::Neutral,
            Self::Initialize | Self::PathMigrated => Status::Warn,
            Self::Pending => Status::Success,
            _ => Status::Error,
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Excluded => ph::PROHIBIT,
            Self::NotInProject => ph::FILE_DASHED,
            Self::Initialize => ph::SEAL_WARNING,
            Self::PathMigrated => ph::GIT_MERGE,
            Self::Pending | Self::UpToDate => ph::FILE,
            Self::Rollback => ph::CLOCK_COUNTER_CLOCKWISE,
            Self::TypeChanged => ph::SHAPES,
            Self::Duplicate => ph::STACK,
            Self::Missing => ph::FILE_X,
        }
    }
}

impl DbPreview {
    pub fn is_desi(&self) -> bool {
        self.db_type == "DESI"
    }

    /// 会不会产生数据批次。阻断的库在执行范围之内、只是这次不跑；
    /// 非 DESI 压根不在范围内——两回事，但都落在「不产生批次」这一边。
    ///
    /// 「够不着」也在这一边，而且它是唯一会**看起来该跑**的一种：MDB 声明了、
    /// 从没导入过，于是契约给的是 `initialization_required = true`。少了这道判断，
    /// 同一行会同时落进确认页的「会执行」与「不会执行」两张卡。
    pub fn will_run(&self) -> bool {
        self.is_desi()
            && !self.not_in_project
            && !self.blocked
            && (!self.sessions.is_empty() || self.initialization_required)
    }

    pub fn changes(&self) -> u32 {
        self.net_added + self.net_modified + self.net_deleted
    }

    pub fn form(&self) -> DbForm {
        if !self.is_desi() {
            return DbForm::Excluded;
        }
        if self.not_in_project {
            return DbForm::NotInProject;
        }
        match &self.anomaly {
            Some(FileAnomaly::Rollback { .. }) => DbForm::Rollback,
            Some(FileAnomaly::TypeChanged { .. }) => DbForm::TypeChanged,
            Some(FileAnomaly::Duplicate { .. }) => DbForm::Duplicate,
            Some(FileAnomaly::Missing { .. }) => DbForm::Missing,
            Some(FileAnomaly::PathMigrated { .. }) => DbForm::PathMigrated,
            None if self.initialization_required => DbForm::Initialize,
            None if self.sessions.is_empty() => DbForm::UpToDate,
            None => DbForm::Pending,
        }
    }

    /// 会话区间。需初始化的库契约不为它解区间，那一行不摆区间而摆一句实话。
    pub fn window(&self) -> String {
        if self.initialization_required {
            return "首次导入 · 尚无权威水位".into();
        }
        format!(
            "sesno {} → {}",
            self.applied_sesno + 1,
            self.file_latest_sesno
        )
    }

    /// 有名有姓的 SITE 桶（归属未知桶不算）。空 = 老服务端没给 `sites[]`，或
    /// 变化全解不出 SITE 祖先——两种情形都退回文件身份行（§2-G 优雅降级）。
    pub fn named_sites(&self) -> impl Iterator<Item = &SitePreview> {
        self.sites.iter().filter(|site| !site.unknown())
    }

    /// 确认页批次行标题（S2-H）：同库 SITE 名单，`SITE /A + /B`。
    /// 没有可说的 SITE 时退回文件身份（首次导入 / 老服务端）。
    pub fn batch_title(&self) -> String {
        let names: Vec<&str> = self.named_sites().map(SitePreview::display_name).collect();
        if names.is_empty() {
            return format!("db{} · {}", self.dbnum, self.db_type);
        }
        format!("SITE {}", names.join(" + "))
    }

    /// SITE 详情行（§2-G 展开首行）。库级事实按 SITE 重复画——这是 SITE 平铺
    /// 明确接受的代价。`exclude` 是当前行自己的 refno，联动名单里不列自己；
    /// 同库只有 1 个 SITE 时整句不画。
    pub fn site_detail_line(&self, exclude: &str) -> String {
        let mut parts = Vec::new();
        if let Some(latest) = self.file_latest_sesno_time.as_deref() {
            let applied = self
                .applied_sesno_time
                .as_deref()
                .map(e3d_time)
                .unwrap_or_else(|| "从未应用".into());
            parts.push(format!(
                "上次应用 {applied} → 文件最新 {}",
                e3d_time(latest)
            ));
        }
        parts.push(format!(
            "待应用 sesno {} → {}（{} 个会话）",
            group_number((self.applied_sesno + 1).into()),
            group_number(self.file_latest_sesno.into()),
            self.sessions.len()
        ));
        let linked: Vec<&str> = self
            .named_sites()
            .map(SitePreview::display_name)
            .filter(|name| *name != exclude)
            .collect();
        if !linked.is_empty() {
            parts.push(format!("与 {} 同库联动", linked.join("、")));
        }
        if matches!(self.anomaly, Some(FileAnomaly::PathMigrated { .. })) {
            parts.push("文件已迁移 · 登记路径自动跟新".into());
        }
        parts.join(" · ")
    }
}

/// RFC3339 → 「MM-DD HH:MM」（本地时区）。语义是 **E3D 会话写入时刻**（ADR-020），
/// 解不动就原样返回——一串错格式比一格假时间诚实。
fn e3d_time(rfc3339: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| rfc3339.to_owned())
}

/// 一次预览能算出来的全部汇总量。分子分母都在这一处算，免得几张画板各算各的。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    pub added: u32,
    pub modified: u32,
    pub deleted: u32,
    pub model_affecting: u32,
    pub no_generation: u32,
    /// 受影响交付单元：`DeliveryUnitSummary` 已按 refno 去重，直接相加。
    pub units: usize,
    /// 其中执行阶段真正会重新生成的单元（`will_generate`）。E3D 的 SAVEWORK
    /// 常会顺带触碰整库元素，产生大量「不生成」单元行——确认页说「将重新生成」
    /// 时必须用这个数，不能用 `units` 总数。
    pub generating: usize,
    /// 纯位姿刷新目标（`transform` 便宜路径，不重建网格）。
    pub transform_targets: usize,
    /// 会产生数据批次的 dbnum 数。
    pub batches: usize,
    /// 在本期执行范围之内的 DESI 库数（当前 MDB 声明、且当前项目目录里有文件）。
    pub desi: usize,
    /// 非 DESI 而被排除的库数。
    pub excluded: usize,
    /// MDB 声明了、但当前项目目录里没有文件的库数。它不算在 `desi` 里——
    /// 两个数摆在一起才回答得了「MDB 说 29 个，这里怎么只有 20 个」。
    pub not_in_project: usize,
    pub blocked: usize,
}

impl Preview {
    pub fn totals(&self) -> Totals {
        let mut t = Totals::default();
        for db in &self.dbnums {
            t.added += db.net_added;
            t.modified += db.net_modified;
            t.deleted += db.net_deleted;
            t.model_affecting += db.model_affecting;
            t.no_generation += db.no_generation;
            t.units += db.units.len();
            t.generating += db.units.iter().filter(|u| u.will_generate).count();
            t.transform_targets += db.transform_targets.len();
            t.batches += usize::from(db.will_run());
            if !db.is_desi() {
                t.excluded += 1;
            } else if db.not_in_project {
                t.not_in_project += 1;
            } else {
                t.desi += 1;
                t.blocked += usize::from(db.blocked);
            }
        }
        t
    }

    /// 本次运行会一并处理的待重试单元。**死信不在其中**——执行侧按服务端的
    /// 重试上限封顶，到顶的行只出现在检查视图里，一次都不会跑。
    pub fn retries_to_run(&self) -> impl Iterator<Item = &PendingModelUnit> {
        self.pending_model_retries.iter().filter(|u| !u.dead)
    }

    /// 已放弃的那些。它们同样在列表里，但要分开数、分开说：混进「将合并」等于
    /// 承诺一件本次不会发生的事。
    pub fn dead_retries(&self) -> impl Iterator<Item = &PendingModelUnit> {
        self.pending_model_retries.iter().filter(|u| u.dead)
    }

    /// 有没有值得按下「开始更新」的东西。待重试单元自己就够成一次运行——
    /// 契约明写没有新 sesno 也要显示并允许重试。
    ///
    /// 但只有**还会跑的**那些算数：一个项目攒下几个死信之后，这里若照旧算进去，
    /// 「开始更新」会永远亮着，按下去扫一圈什么也不做，而死信一个都没动。
    pub fn executable(&self) -> bool {
        self.dbnums.iter().any(DbPreview::will_run) || self.retries_to_run().next().is_some()
    }
}

// ================================================================ 执行契约

/// `POST /api/v1/update/execute` 的 202 回执，与 gen-model 的 `ManualEnqueueReceipt` 同形。
///
/// 合流之后执行请求**一律入队**，回的不再是单个 task_id：一次扫描可能排下好几个库，
/// 也可能一行都没排（都并进了既有排队行，或者水位本来就最新）。「已有任务在跑 · 409」
/// 那一格随之退役——执行请求不会再被拒。进度去「任务队列」视图看。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Enqueued {
    #[serde(default)]
    pub project: String,
    /// 本次扫描到的候选 dbnum 数（含已是最新的与被阻断的）。
    #[serde(default)]
    pub scanned: usize,
    /// 新排的行，含接在运行中批次之后的那种。
    #[serde(default)]
    pub enqueued: Vec<EnqueuedBatch>,
    /// 并入既有排队行的：目标会话号被推高，不另开一行。
    #[serde(default)]
    pub merged: Vec<EnqueuedBatch>,
    /// 已被既有排队行覆盖、无需动作的 dbnum。
    #[serde(default)]
    pub already_covered: Vec<u32>,
    /// 阻断的库压根不入队，队列里不会有它们的行——这里是唯一说得出
    /// 「它为什么没排上」的地方。
    #[serde(default)]
    pub blocked: Vec<BlockedDbnum>,
    #[serde(default)]
    pub up_to_date: usize,
    /// 请求带了 `dbnums` 勾选子集（ADR-020）而未勾选的库：不扫描、不入队、
    /// 水位不动，等下一次执行。全范围请求时恒为空。
    #[serde(default)]
    pub unselected: Vec<u32>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct EnqueuedBatch {
    pub task_id: String,
    pub dbnum: u32,
    #[serde(default)]
    pub db_type: String,
    /// 排队中的位置（1 起；运行中的行不占位置）。
    #[serde(default)]
    pub position: usize,
    #[serde(default)]
    pub start_sesno: i32,
    #[serde(default)]
    pub end_sesno: i32,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BlockedDbnum {
    pub dbnum: u32,
    #[serde(default)]
    pub reason: String,
}

impl Enqueued {
    /// 一句能写进日志的回执。五个去向分开说——它们的出路各不相同：排上的等着跑、
    /// 并入的已经在别人那一行里、被覆盖的无需动作、已是最新的本来就没事、
    /// 阻断的要人去处理文件。
    ///
    /// **五个数要与 `scanned` 算得平。** 早先的写法只在「一行都没排」那条兜底
    /// 分支里报 `up_to_date` 与 `already_covered`，于是只要有任意一条入队，
    /// 那两个数就整个消失：人按下确认看到「扫描 29 个库；已入队 2 个数据批次」，
    /// 剩下 27 个去哪了说不出来。那与本仓自己坚持的「排除有两个理由，界面上必须
    /// 分开数」「过滤不许无声」正好相反。
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.enqueued.is_empty() {
            let list = self
                .enqueued
                .iter()
                .map(|b| format!("db{} 排第 {} 位", b.dbnum, b.position))
                .collect::<Vec<_>>()
                .join("、");
            parts.push(format!(
                "已入队 {} 个数据批次（{list}）",
                self.enqueued.len()
            ));
        }
        if !self.merged.is_empty() {
            let list = self
                .merged
                .iter()
                .map(|b| format!("db{}", b.dbnum))
                .collect::<Vec<_>>()
                .join("、");
            parts.push(format!(
                "{} 个并入已排队的批次（{list}）",
                self.merged.len()
            ));
        }
        if !self.already_covered.is_empty() {
            let list = self
                .already_covered
                .iter()
                .map(|dbnum| format!("db{dbnum}"))
                .collect::<Vec<_>>()
                .join("、");
            parts.push(format!(
                "{} 个已被排队中的批次覆盖（{list}）",
                self.already_covered.len()
            ));
        }
        if self.up_to_date > 0 {
            parts.push(format!("{} 个已是最新", self.up_to_date));
        }
        if !self.unselected.is_empty() {
            let list = self
                .unselected
                .iter()
                .map(|dbnum| format!("db{dbnum}"))
                .collect::<Vec<_>>()
                .join("、");
            parts.push(format!(
                "{} 个未勾选跳过（{list}，水位不变）",
                self.unselected.len()
            ));
        }
        if !self.blocked.is_empty() {
            let list = self
                .blocked
                .iter()
                .map(|b| format!("db{}：{}", b.dbnum, b.reason))
                .collect::<Vec<_>>()
                .join("；");
            parts.push(format!("{} 个阻断未入队（{list}）", self.blocked.len()));
        }
        if parts.is_empty() {
            return format!(
                "扫描 {} 个库，没有需要入队的批次，回执也没说这些库各自是什么去向",
                self.scanned
            );
        }
        // 加不到 `scanned` 就把差额摆出来。契约上这五桶应当是它的一个划分，
        // 但那是服务端的性质，不是本端能保证的事——差额说不出来的时候，
        // 宁可承认「有 N 个库没交代」也不让那个数无声消失。
        let unaccounted = self.scanned.saturating_sub(self.accounted());
        if unaccounted > 0 {
            parts.push(format!("另有 {unaccounted} 个库回执没交代去向"));
        }
        format!("扫描 {} 个库；{}", self.scanned, parts.join("；"))
    }

    /// 回执把去向说清楚了的库数。与 `scanned` 的差就是说不清的那部分。
    fn accounted(&self) -> usize {
        self.enqueued.len()
            + self.merged.len()
            + self.already_covered.len()
            + self.up_to_date
            + self.unselected.len()
            + self.blocked.len()
    }
}

/// 一个数据批次的结果摘要，与 gen-model 的 `DataBatchTaskResult` 同形。
///
/// **`batch` 是单数**：合流之后一个任务就是一个数据批次，「一次运行装着几个批次」
/// 那种形状随「运行」一并退役（ADR-0011）。解成数组的话字段名对不上、又带
/// `serde(default)`，会静默解成空——界面上看不出任何异样，摘要却整块空掉。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Outcome {
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub status: RunStatus,
    /// 排队行被冻结重扫完全覆盖、未单独执行（gen-model ADR-011 §5）。
    /// 新服务端在 absorbed 收口时带出这个标志；旧服务端以
    /// `status = absorbed_by_running` 表达同一件事，判读走 [`Self::is_absorbed`]。
    #[serde(default)]
    pub absorbed: bool,
    #[serde(default)]
    pub batch: Option<BatchResult>,
    #[serde(default)]
    pub units: Vec<UnitResult>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// 一个成功生成的交付单元给出的刷新线索。
///
/// 更新之后可能变的有三处：单元自己（几何换了）、它更新**前**的 OWNER（元素可能
/// 从那儿移走或被删掉）、更新**后**的 OWNER（元素可能是新增或移进去的）。三者
/// 常常是同一个，两个 OWNER 也都可能取不到——契约里它们是可选字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshUnit {
    pub root: RefU64,
    pub old_owner: Option<RefU64>,
    pub new_owner: Option<RefU64>,
}

impl Outcome {
    pub fn data_applied(&self) -> bool {
        self.batch
            .as_ref()
            .is_some_and(|batch| batch.status == BatchStatus::Applied && batch.changed_elements > 0)
    }

    /// 这一行入队后被运行中批次覆盖、没有单独执行（ADR-011 §5 的 absorbed 收口）。
    /// 两代服务端两种说法，这里收拢成一个判读：新的给 `absorbed = true`（status 是
    /// `up_to_date`），旧的把 `status` 本身写成 `absorbed_by_running`。
    pub fn is_absorbed(&self) -> bool {
        self.absorbed || self.status == RunStatus::AbsorbedByRunning
    }

    /// 本次真正生成成功的交付单元及其原 / 新 OWNER。
    ///
    /// 失败单元不进来：它们的模型压根没换，旧显示仍然是对的。解不出参考号的也
    /// 跳过——契约给的是 PDMS 串，宁可少刷一个分支也不去猜它想说什么。
    pub fn refresh_units(&self) -> Vec<RefreshUnit> {
        fn parse(text: &str) -> Option<RefU64> {
            RefU64::from_str(text).ok().filter(|r| r.is_valid())
        }
        self.units
            .iter()
            .filter(|unit| unit.status == UnitStatus::Generated)
            .filter_map(|unit| {
                Some(RefreshUnit {
                    root: parse(&unit.root_refno)?,
                    old_owner: unit.old_owner.as_deref().and_then(parse),
                    new_owner: unit.new_owner.as_deref().and_then(parse),
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Success,
    /// 既有成功也有失败。数据批次已推进的水位不因模型生成失败而回退。
    Partial,
    Failed,
    #[default]
    UpToDate,
    /// 旧服务端的 absorbed 收口（gen-model ADR-011 §5）：排队行被冻结重扫完全
    /// 覆盖，工作在运行中的那一批里完成，这一行没有单独执行。新服务端不再用
    /// 这个值（改发 `up_to_date` + `absorbed: true`），留着它是为了旧服务端。
    AbsorbedByRunning,
    /// 认不出的终态兜底。没有它，服务端多一种 result 形状就毒化整张
    /// `/tasks` 列表（一条解不动，`Vec<TaskEntry>` 整个失败），队列面板会
    /// 冻在旧快照上直到那一行被挤出 200 条窗口——宁可认不出，不许连坐。
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BatchResult {
    pub dbnum: u32,
    #[serde(default)]
    pub db_type: String,
    #[serde(default)]
    pub file_path: String,
    pub start_sesno: i32,
    pub end_sesno: i32,
    /// 窗口两端那两条保存的**写入时刻**（RFC3339）。终态行内明细摆的是这一对，
    /// 序号只留作执行边界（ADR-0019 Q3）。任一端缺席就整格不画，不回落成 sesno。
    #[serde(default)]
    pub start_sesno_time: Option<String>,
    /// 右端时刻。「水位推进至」那一段说的也是它（Q8），与右端重复是刻意的。
    #[serde(default)]
    pub end_sesno_time: Option<String>,
    #[serde(default)]
    pub status: BatchStatus,
    #[serde(default)]
    pub message: Option<String>,
    /// 预览扫描之后才并入本批次的会话。契约明写结果摘要必须逐条列出。
    #[serde(default)]
    pub merged_sesnos: Vec<u32>,
    /// 与 `merged_sesnos` **一一对应**的写入时刻（ADR-0019 Q5）。
    ///
    /// 服务端保证等长，按下标配对；读不到的那条是 `None`。**别按长度对齐**——
    /// 老服务端根本不给这个字段，那时它是空数组而 `merged_sesnos` 有内容。
    #[serde(default)]
    pub merged_sesno_times: Vec<Option<String>>,
    #[serde(default)]
    pub changed_elements: u64,
}

impl BatchResult {
    /// 并入的那几条保存，逐条配上时刻（ADR-0019 Q5）。
    ///
    /// **只有每一条都拿得到时刻才给**：漏掉几条却仍标着「并入 3 次」会让人以为
    /// 列出来的就是全部。缺任何一条就返回 `None`，由调用点只报条数。
    pub fn merged_save_times(&self) -> Option<Vec<&str>> {
        if self.merged_sesno_times.len() != self.merged_sesnos.len() {
            return None;
        }
        self.merged_sesno_times
            .iter()
            .map(|time| time.as_deref())
            .collect()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    Applied,
    Failed,
    #[default]
    Skipped,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnitResult {
    pub dbnum: u32,
    pub root_refno: String,
    #[serde(default)]
    pub noun: String,
    #[serde(default)]
    pub status: UnitStatus,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub old_owner: Option<String>,
    #[serde(default)]
    pub new_owner: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitStatus {
    #[default]
    Generated,
    Failed,
}

/// 执行期的两阶段进度事件，与 gen-model 的 `ManualUpdateEvent` 同形。
/// 只在 `/api/v1/ws` 的 tasks 主题上发，轮询取不到（ADR-0005）。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProgressEvent {
    DataBatchStarted {
        dbnum: u32,
        start_sesno: i32,
        end_sesno: i32,
    },
    DataBatchFinished {
        dbnum: u32,
        success: bool,
        message: Option<String>,
    },
    ModelUnitStarted {
        dbnum: u32,
        root_refno: String,
        noun: String,
    },
    ModelUnitFinished {
        dbnum: u32,
        root_refno: String,
        success: bool,
        message: Option<String>,
    },
}

/// 一行的进行状态。`Unknown` 专给断线：没收到事件就不知道它开没开始，
/// 不许说成「排队中」（ADR-0005）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Running,
    Done,
    Failed,
    Unknown,
}

/// WebSocket 那条通道自己的状态。它与任务状态是两件事：连接断了任务照跑，
/// 所以要分开显示，不能拿连接状态去暗示任务出了问题。
/// 消费者是任务队列视图——向导没有长连接（执行进度不在这儿，ADR-0011）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Feed {
    #[default]
    Connecting,
    Live,
    /// 断开，附一句能说给人听的原因。
    Down(String),
    /// 这个构建**压根不订阅**逐单元明细（wasm 端就是这样：进度走轮询）。
    ///
    /// 它与 `Down` 不是一回事，不许合成一个。断线是「连过、掉了、可以重连、
    /// 中间那段确实漏了」；这一种是从来没连过——说「断线期间没收到事件」是在
    /// 指控一次不存在的故障，摆一个「明细缺 N 条」更是把「本来就不收」讲成
    /// 「丢了」，而那个数恰好等于服务端发过的全部。
    NotSubscribed(String),
}

// ================================================================ 不可用与失败态

/// 服务端错误包封 `{ code, message, detail }`。**按 `code` 分型，不解析 message 字符串，
/// 更不把 anyhow 错误链摊给人看**（S2-D）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Failure {
    pub code: String,
    pub message: String,
    pub detail: Option<String>,
}

/// S2-D 的失败形态，减去「已是最新」（那是 200，走 `up_to_date`）与
/// 「数据源未就绪」（那一态连请求都不发，入口按钮已经禁着）。
///
/// 「自动同步接管 · 422」与「已有任务在跑 · 409」两种形态随合流退役
/// （gen-model ADR-011 §12：单飞预检与 `sync_live` 拒绝全部删除，执行请求
/// 一律 202 入队）。意外冒出来的这两个 code 归 `Internal`——一个说不清来路的
/// 拒绝，把原始 message 收进详情比替它编一个已不存在的理由诚实。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailForm {
    /// 422 · identity_mismatch：请求的 project / MDB / namespace 与模型服务的
    /// 固定范围不一致。服务端 `ServiceIdentity::validate` 会给出它，本地闸门
    /// （`task_queue::Vm::can_mutate`）拦下时也用同一个 code 合成，两处一个画面。
    IdentityMismatch,
    /// 504 · timeout：连不上或超时。
    Timeout,
    /// 422 · worker_disabled：服务以 direct 形态运行、没起数据批次 worker，执行请求
    /// 入队后没人出队。本地闸门（`task_queue::Vm::can_execute`）按 `/health` 拦下时
    /// 也用同一个 code 合成；服务端若在 direct 下直接拒绝（gen-model G1 的备选路径）
    /// 也走这一格。预览不受影响——它不需要 worker。
    WorkerDisabled,
    /// 其余一律 500 · internal——**只有这一类**才需要把原始 message 摊出来。
    Internal,
}

impl Failure {
    pub fn form(&self) -> FailForm {
        match self.code.as_str() {
            "identity_mismatch" => FailForm::IdentityMismatch,
            "timeout" => FailForm::Timeout,
            "worker_disabled" => FailForm::WorkerDisabled,
            _ => FailForm::Internal,
        }
    }

    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            detail: None,
        }
    }
}

impl FailForm {
    fn title(self) -> &'static str {
        match self {
            Self::IdentityMismatch => "范围与模型服务对不上",
            Self::Timeout => "连不上模型服务",
            Self::WorkerDisabled => "模型服务此刻不消化执行请求",
            Self::Internal => "模型服务出错",
        }
    }

    fn body(self) -> &'static str {
        match self {
            Self::IdentityMismatch => {
                "请求的 project / MDB / namespace 与模型服务的固定范围不一致。确认当前数据源与模型服务连的是同一套项目配置，再重新预览。"
            }
            Self::Timeout => "模型服务没有响应。没有任何数据被改动，可以直接重试。",
            Self::WorkerDisabled => {
                "模型服务以 direct 形态运行，没有启动数据批次 worker：预览照常，但执行请求入队后不会被处理。没有任何数据被改动。等服务端切换形态或联系管理员后再试。"
            }
            Self::Internal => "服务端未能完成这次请求。原始信息收在下面的详情里。",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::IdentityMismatch => ph::LINK_BREAK,
            Self::Timeout => ph::WARNING,
            Self::WorkerDisabled => ph::PAUSE_CIRCLE,
            Self::Internal => ph::WARNING_CIRCLE,
        }
    }

    fn tone(self) -> Status {
        match self {
            Self::IdentityMismatch | Self::WorkerDisabled => Status::Warn,
            _ => Status::Error,
        }
    }
}

// ================================================================ 视图状态

#[derive(Debug, Clone, Default)]
pub enum Vm {
    /// 还没预览，或者用户放弃了等待。
    #[default]
    Idle,
    Loading,
    Ready(Preview),
    /// 正在把这份预览重新扫描并入队。保留它，供终态行显示“预览时 N 项”。
    Starting(Preview),
    Failed(Failure),
}

impl Vm {
    /// 相位标识。计时器靠它判断该不该重新起表；同一相位内数据怎么变都不重置。
    fn phase(&self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Loading => 1,
            Self::Ready(_) => 2,
            Self::Starting(_) => 3,
            Self::Failed(_) => 4,
        }
    }
}

/// 向导走到第几步。两步都是纯界面状态：「确认执行」不发任何请求，它存在的
/// 唯一理由是**按下去的是一个不可取消的按钮**。确认之后的执行进度在任务队列
/// 视图（ADR-0011），不在本窗。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Step {
    #[default]
    Preview,
    Confirm,
}

impl Step {
    fn index(self) -> usize {
        match self {
            Self::Preview => 0,
            Self::Confirm => 1,
        }
    }
}

/// 缓存的上一份预览。S2-A 的等待态摆它的过期摘要；标题栏在 Idle / Loading /
/// Failed 时靠它报项目名。
struct Cached {
    preview: Preview,
    at: Instant,
}

/// 相位计时器。「已用 01:12」是本地量，从相位切换那一刻起算。
#[derive(Default)]
struct Clock {
    phase: u8,
    since: Option<Instant>,
}

impl Clock {
    fn sync(&mut self, phase: u8) {
        if self.phase != phase || self.since.is_none() {
            self.phase = phase;
            self.since = Some(Instant::now());
        }
    }

    fn elapsed(&self) -> Duration {
        self.since.map(|s| s.elapsed()).unwrap_or_default()
    }
}

#[derive(Default)]
pub struct State {
    pub open: bool,
    step: Step,
    clock: Clock,
    cache: Option<Cached>,
    /// 显式点过的展开箭头。默认展开与否按行的种类定，这里只记「与默认相反」的那些。
    toggled: HashSet<String>,
    /// 勾选集的补集（§2-G：默认全勾，记的是被取消的那些）。键是 dbnum——
    /// **同库联动**（Q1 定案）：勾任何一个 SITE = 同库 SITE 全亮，只有勾 / 不勾
    /// 两态、没有半选，所以状态天然按库记，不按 SITE 记。
    unchecked: HashSet<u32>,
    only_model_affecting: bool,
    /// internal 那一类失败的原始 message，默认折起。
    detail_open: bool,
}

impl State {
    fn sync(&mut self, vm: &Vm) {
        let phase = vm.phase();
        let switched = self.clock.phase != phase;
        self.clock.sync(phase);
        match vm {
            Vm::Ready(preview) => {
                if switched || self.cache.is_none() {
                    self.cache = Some(Cached {
                        preview: preview.clone(),
                        at: Instant::now(),
                    });
                }
                if switched {
                    // 预览一刷新就退回第一步：确认页上摆的是刚才那份的数字。
                    // 勾选集同时归零（回到默认全勾）——旧勾选对着的是上一份预览。
                    self.step = Step::Preview;
                    self.detail_open = false;
                    self.unchecked.clear();
                }
            }
            Vm::Failed(_) if switched => self.detail_open = false,
            _ => {}
        }
    }

    fn open_row(&self, key: &str, default_open: bool) -> bool {
        self.toggled.contains(key) != default_open
    }

    fn toggle_row(&mut self, key: &str) {
        if !self.toggled.remove(key) {
            self.toggled.insert(key.to_owned());
        }
    }

    fn db_checked(&self, dbnum: u32) -> bool {
        !self.unchecked.contains(&dbnum)
    }

    fn toggle_db(&mut self, dbnum: u32) {
        if !self.unchecked.remove(&dbnum) {
            self.unchecked.insert(dbnum);
        }
    }

    /// 勾选集折算成 execute 请求的 `dbnums[]`（ADR-020）：勾中的可执行库号，
    /// 含首次导入库。**总是显式给名单**——预览之后新冒出来的库不在这份确认里，
    /// 不该被顺手执行。
    fn selected_dbnums(&self, preview: &Preview) -> Vec<u32> {
        preview
            .dbnums
            .iter()
            .filter(|db| db.will_run() && self.db_checked(db.dbnum))
            .map(|db| db.dbnum)
            .collect()
    }

    fn cached(&self) -> Option<&Preview> {
        self.cache.as_ref().map(|c| &c.preview)
    }
}

/// 勾选集折算（§2-G 工具行 / 主按钮 / 确认页共用同一份算法，免得三处各算各的）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Selection {
    /// 有变化的 SITE 总数（有名有姓的桶；归属未知桶与首次导入库不算 SITE）。
    site_total: usize,
    site_checked: usize,
    /// 勾中折算出的数据批次数（含勾中的首次导入库）。
    batches: usize,
    batches_total: usize,
    /// 勾中批次的交付单元数（按 `db.units` 去重口径，不用 SITE 桶——挪动的
    /// 单元在两个桶里各出现一次，按桶相加会虚高）。
    units: usize,
    generating: usize,
    transform_targets: usize,
}

fn selection(preview: &Preview, state: &State) -> Selection {
    let mut s = Selection::default();
    for db in &preview.dbnums {
        if !db.will_run() {
            continue;
        }
        let named = db.named_sites().count();
        s.batches_total += 1;
        s.site_total += named;
        if state.db_checked(db.dbnum) {
            s.batches += 1;
            s.site_checked += named;
            s.units += db.units.len();
            s.generating += db.units.iter().filter(|u| u.will_generate).count();
            s.transform_targets += db.transform_targets.len();
        }
    }
    s
}

// ================================================================ 窗口外壳

/// `applying` 是此刻正在应用的数据批次数（队列快照里本项目 `running` 的行，
/// 由 `task_queue::Vm::applying` 算出）。预览与批次并发时，正在被应用的会话会被
/// 算进「待应用」，S2 拿它标注「N 个库正在应用，数字可能偏大」（ADR-0011）。
///
/// `execution_blocked` 是「执行请求此刻为什么会白点」（`task_queue::Vm::execution_blocked_reason`）：
/// `Some` 时「开始更新」与「确认并开始」灰掉并把原因摆在旁边；预览不受影响——
/// 它不需要 worker 在场。
#[allow(clippy::too_many_arguments)]
pub fn show(
    ctx: &egui::Context,
    t: &Tokens,
    d: Density,
    api_url: &str,
    applying: usize,
    scope_mdb: &str,
    sync_live: Option<bool>,
    execution_blocked: Option<&str>,
    vm: &Vm,
    state: &mut State,
) -> Vec<Cmd> {
    if !state.open {
        return Vec::new();
    }
    state.sync(vm);

    let mut cmds = Vec::new();
    egui::Window::new("模型增量更新")
        .id(egui::Id::new("model-update-window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(true)
        .default_size([d.px(1040.0), d.px(720.0)])
        .min_size([d.px(760.0), d.px(520.0)])
        .frame(
            egui::Frame::new()
                .fill(t.bg_panel)
                .corner_radius(CornerRadius::same(radius::LG))
                .stroke(Stroke::new(1.0_f32, t.border_strong)),
        )
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            header(ui, t, d, scope_mdb, sync_live, vm, state);
            steps(ui, t, d, state.step);

            let footer_h = d.px(56.0);
            let body_h = (ui.available_height() - footer_h).max(d.px(200.0));
            let width = ui.available_width();
            ui.allocate_ui(vec2(width, body_h), |ui| {
                ui.set_min_size(vec2(width, body_h));
                match vm {
                    Vm::Idle => idle_body(ui, t, d),
                    Vm::Loading => previewing_body(ui, t, d, state),
                    Vm::Starting(_) => center_note(
                        ui,
                        t,
                        d,
                        None,
                        "正在提交增量更新任务…",
                        "服务端接下任务后，本窗会关闭并切到任务队列。",
                    ),
                    Vm::Failed(failure) => failure_body(ui, t, d, failure, state),
                    Vm::Ready(preview) if preview.up_to_date => up_to_date_body(ui, t, d, state),
                    Vm::Ready(preview) => match state.step {
                        Step::Confirm => confirm_body(ui, t, d, preview, state),
                        _ => preview_body(ui, t, d, applying, preview, state),
                    },
                }
            });

            footer(
                ui,
                t,
                d,
                footer_h,
                api_url,
                execution_blocked,
                vm,
                state,
                &mut cmds,
            );
        });
    cmds
}

fn header(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    scope_mdb: &str,
    sync_live: Option<bool>,
    vm: &Vm,
    state: &mut State,
) {
    let (title, sub, tone, icon) = header_text(vm, state, scope_mdb, sync_live);
    let rect = bar(ui, d.px(52.0), t.bg_chrome, Corner::Top);
    let mut ui = child(ui, rect.shrink2(vec2(d.px(16.0), 0.0)), Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(12.0);

    let (fg, bg) = t.status(tone);
    let (chip, _) = ui.allocate_exact_size(vec2(d.px(28.0), d.px(28.0)), Sense::hover());
    fill(&ui, chip, radius::MD, bg, None);
    glyph(&ui, chip.center(), icon, d.px(15.0), fg);

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(
            RichText::new(title)
                .font(Font::title(d))
                .color(t.text_primary),
        );
        ui.label(RichText::new(sub).font(Font::meta(d)).color(t.text_muted));
    });

    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if ui.add(widgets::tool_btn(t, d, ph::X, false)).clicked() {
            state.open = false;
        }
        // 「已用」只在预览扫描在途时摆；其余相位摆一句用不着计时的提示。
        match vm {
            Vm::Loading => stopwatch(ui, t, d, "已用", state.clock.elapsed()),
            _ => {
                ui.label(
                    RichText::new("关闭窗口不会中断任务")
                        .font(Font::meta(d))
                        .color(t.text_muted),
                );
            }
        }
    });
}

fn header_text(
    vm: &Vm,
    state: &State,
    scope_mdb: &str,
    sync_live: Option<bool>,
) -> (String, String, Status, &'static str) {
    let project = project_of(vm, state);
    let identity = |mdb: &str| {
        let mut parts = vec![project.clone()];
        if !mdb.is_empty() {
            parts.push(format!("MDB {mdb}"));
        }
        if let Some(live) = sync_live {
            parts.push(if live {
                "自动同步已开启".to_owned()
            } else {
                "自动同步已关闭".to_owned()
            });
        }
        parts.join(" · ")
    };
    match vm {
        Vm::Idle => (
            "模型更新".into(),
            format!("{} · 尚未预览", identity(scope_mdb)),
            Status::Neutral,
            ph::ARROWS_CLOCKWISE,
        ),
        Vm::Loading => (
            "模型更新 · 正在预览".into(),
            format!("{} · 只读扫描", identity(scope_mdb)),
            Status::Info,
            ph::ARROWS_CLOCKWISE,
        ),
        Vm::Starting(preview) => (
            "模型更新 · 正在提交".into(),
            format!("{} · 等待服务端接下任务", identity(&preview.mdb)),
            Status::Info,
            ph::ARROWS_CLOCKWISE,
        ),
        Vm::Failed(failure) => (
            format!("模型更新 · {}", failure.form().title()),
            format!("{} · {}", identity(scope_mdb), failure.code),
            failure.form().tone(),
            failure.form().icon(),
        ),
        Vm::Ready(preview) if preview.up_to_date => (
            "模型更新 · 已是最新".into(),
            format!("{} · 没有可更新的内容", identity(&preview.mdb)),
            Status::Success,
            ph::CHECK_CIRCLE,
        ),
        Vm::Ready(preview) if state.step == Step::Confirm => (
            "模型更新 · 确认执行".into(),
            format!("{} · 开始后不可中途取消", identity(&preview.mdb)),
            Status::Warn,
            ph::SEAL_WARNING,
        ),
        // 范围由 MDB 定，副标题就得把 MDB 说出来——服务端与客户端各有一份
        // `mdb_name`，只说「仅扫描当前项目」的话，两边错开时界面一声不响。
        Vm::Ready(preview) => (
            "模型更新".into(),
            identity(&preview.mdb),
            Status::Info,
            ph::ARROWS_CLOCKWISE,
        ),
    }
}

fn project_of(vm: &Vm, state: &State) -> String {
    match vm {
        Vm::Ready(preview) | Vm::Starting(preview) if !preview.project.is_empty() => {
            preview.project.clone()
        }
        _ => state
            .cached()
            .map(|p| p.project.clone())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| "当前项目".into()),
    }
}

/// 步骤条。两步是本仓库定义的流程，没有契约来源；执行进度不是一步——
/// 它在任务队列视图（ADR-0011）。
fn steps(ui: &mut Ui, t: &Tokens, d: Density, step: Step) {
    const NAMES: [&str; 2] = ["更新预览", "确认执行"];
    let rect = bar(ui, d.px(48.0), t.bg_panel, Corner::None);
    let mut ui = child(ui, rect.shrink2(vec2(d.px(20.0), 0.0)), Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(8.0);

    let current = step.index();
    // 连接线均分标签之外的剩余宽度。先量总宽再分，边量边分的话后一条会比前一条短一截。
    let labels: f32 = NAMES
        .iter()
        .map(|n| layout(&ui, n, Font::strong(d)).size().x + d.px(28.0))
        .sum();
    let connector =
        ((ui.available_width() - labels) / (NAMES.len() - 1) as f32 - d.px(24.0)).max(d.px(24.0));

    for (i, name) in NAMES.into_iter().enumerate() {
        if i > 0 {
            let (line, _) = ui.allocate_exact_size(vec2(connector, 1.0), Sense::hover());
            ui.painter().rect_filled(
                line,
                CornerRadius::ZERO,
                if i <= current { t.success } else { t.border },
            );
        }
        let (num, _) = ui.allocate_exact_size(vec2(d.px(20.0), d.px(20.0)), Sense::hover());
        if i < current {
            glyph(&ui, num.center(), ph::CHECK, d.px(14.0), t.success);
        } else if i == current {
            ui.painter()
                .circle_filled(num.center(), d.px(10.0), t.accent);
            mono_at(&ui, num.center(), &(i + 1).to_string(), d, t.accent_ink);
        } else {
            ui.painter().circle_stroke(
                num.center(),
                d.px(10.0) - 0.5,
                Stroke::new(1.0_f32, t.border_strong),
            );
            mono_at(&ui, num.center(), &(i + 1).to_string(), d, t.text_muted);
        }
        let (font, color) = if i == current {
            (Font::strong(d), t.text_primary)
        } else if i < current {
            (Font::body(d), t.text_secondary)
        } else {
            (Font::body(d), t.text_muted)
        };
        ui.label(RichText::new(name).font(font).color(color));
    }
    hairline(ui.painter(), rect, t.border);
}

#[allow(clippy::too_many_arguments)]
fn footer(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    h: f32,
    api_url: &str,
    execution_blocked: Option<&str>,
    vm: &Vm,
    state: &mut State,
    cmds: &mut Vec<Cmd>,
) {
    let rect = bar(ui, h, t.bg_chrome, Corner::Bottom);
    ui.painter().rect_filled(
        Rect::from_min_size(rect.left_top(), vec2(rect.width(), 1.0)),
        CornerRadius::ZERO,
        t.border,
    );
    let mut ui = child(ui, rect.shrink2(vec2(d.px(16.0), 0.0)), Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(8.0);

    let (icon, hint, tone) = footer_hint(vm, state, api_url, execution_blocked);
    let (fg, _) = t.status(tone);
    let (mark, _) = ui.allocate_exact_size(vec2(d.px(14.0), d.px(14.0)), Sense::hover());
    glyph(&ui, mark.center(), icon, d.px(13.0), fg);
    ui.label(
        RichText::new(hint)
            .font(Font::meta(d))
            .color(if tone == Status::Neutral {
                t.text_muted
            } else {
                fg
            }),
    );

    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        footer_actions(ui, t, d, execution_blocked, vm, state, cmds);
    });
}

fn footer_hint(
    vm: &Vm,
    state: &State,
    api_url: &str,
    execution_blocked: Option<&str>,
) -> (&'static str, String, Status) {
    match vm {
        Vm::Loading => (
            ph::LOCK_SIMPLE,
            "预览为只读：不修改元素数据、模型或已应用会话号；关闭窗口不影响后台扫描".into(),
            Status::Neutral,
        ),
        // 服务此刻不消化执行请求：这句要顶掉「预览为只读」——人对着一份预览、
        // 按钮却灰着，唯一用得上的信息就是为什么。
        Vm::Ready(preview) if !preview.up_to_date && execution_blocked.is_some() => (
            ph::PAUSE_CIRCLE,
            format!(
                "{}。预览照常可看，执行要等服务端",
                execution_blocked.unwrap_or_default()
            ),
            Status::Warn,
        ),
        Vm::Ready(preview) if !preview.up_to_date && state.step == Step::Confirm => (
            ph::WARNING,
            "确认后立即开始，任务不可中途取消".into(),
            Status::Warn,
        ),
        Vm::Ready(preview) if !preview.up_to_date => (
            ph::LOCK_SIMPLE,
            "预览为只读：不修改元素数据、模型或已应用会话号".into(),
            Status::Neutral,
        ),
        _ => (ph::PLUGS, format!("服务 {api_url}"), Status::Neutral),
    }
}

fn footer_actions(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    execution_blocked: Option<&str>,
    vm: &Vm,
    state: &mut State,
    cmds: &mut Vec<Cmd>,
) {
    match vm {
        Vm::Idle | Vm::Failed(_) => {
            if ui
                .add(widgets::button(t, d, "重新预览").icon(ph::ARROWS_CLOCKWISE))
                .clicked()
            {
                cmds.push(Cmd::RefreshModelUpdate);
            }
            if ui.add(widgets::button(t, d, "关闭")).clicked() {
                state.open = false;
            }
        }
        Vm::Loading => {
            // 「取消预览」只放弃客户端等待。服务端没有 cancel 接口，文案不许暗示它会停。
            if ui.add(widgets::button(t, d, "取消预览")).clicked() {
                cmds.push(Cmd::CancelModelUpdatePreview);
            }
        }
        Vm::Starting(_) => {}
        Vm::Ready(preview) if preview.up_to_date => {
            if ui
                .add(widgets::button(t, d, "重新预览").icon(ph::ARROWS_CLOCKWISE))
                .clicked()
            {
                cmds.push(Cmd::RefreshModelUpdate);
            }
            if ui.add(widgets::button(t, d, "关闭")).clicked() {
                state.open = false;
            }
        }
        Vm::Ready(preview) => {
            // 批次数按勾选集折算（§2-G 主按钮），不再是全部可执行库数。
            let sel = selection(preview, state);
            if state.step == Step::Confirm {
                // 服务此刻不消化执行请求（direct 无 worker / worker 已死）：确认键灰掉。
                // 人在确认页上等着服务端起来，这里的判定随每拍 /health 变，不用退回预览。
                if ui
                    .add_enabled(
                        execution_blocked.is_none(),
                        widgets::button(t, d, &format!("确认并开始 · {} 个数据批次", sel.batches))
                            .icon(ph::PLAY)
                            .primary(),
                    )
                    .on_disabled_hover_text(execution_blocked.unwrap_or_default())
                    .clicked()
                {
                    // 勾选集折算成 execute 的 `dbnums[]`（ADR-020）。总是显式给
                    // 名单：预览之后新冒出来的库不在这份确认里，不该被顺手执行。
                    cmds.push(Cmd::ExecuteModelUpdate {
                        dbnums: Some(state.selected_dbnums(preview)),
                    });
                }
                if ui.add(widgets::button(t, d, "返回预览")).clicked() {
                    state.step = Step::Preview;
                }
            } else {
                // 一个批次都没勾时，待重试单元自己也够成一次运行（契约明写没有
                // 新 sesno 也要显示并允许重试）——死信不算。
                let has_work = sel.batches > 0 || preview.retries_to_run().next().is_some();
                let executable = has_work && execution_blocked.is_none();
                if ui
                    .add_enabled(
                        executable,
                        widgets::button(t, d, &format!("开始更新 · {} 个批次", sel.batches))
                            .icon(ph::PLAY)
                            .primary(),
                    )
                    .on_disabled_hover_text(
                        // 先说服务不消化——那才是真正的原因；有活可干但服务不接，
                        // 「没有勾选的批次」这句就是在说反话。
                        execution_blocked.unwrap_or("没有勾选的数据批次，也没有待重试单元"),
                    )
                    .clicked()
                {
                    state.step = Step::Confirm;
                }
                if ui.add(widgets::button(t, d, "取消")).clicked() {
                    state.open = false;
                }
            }
        }
    }
}

// ================================================================ 第一步：预览

fn idle_body(ui: &mut Ui, t: &Tokens, d: Density) {
    center_note(
        ui,
        t,
        d,
        Some(ph::ARROWS_CLOCKWISE),
        "还没有预览结果",
        "预览只读扫描设计库，不改任何数据。按下方「重新预览」开始。",
    );
}

/// S2-A 正在预览。逐库推进要等 ADR-0006 的 `PreviewDbStarted / Finished` 事件，
/// 现在一条都没有，所以这里不摆库计数、不摆进度条——**造一条走不满的条比不摆更糟**。
fn previewing_body(ui: &mut Ui, t: &Tokens, d: Density, state: &State) {
    pad(ui, d, |ui| {
        ui.spacing_mut().item_spacing.y = d.px(12.0);
        card(ui, t, d, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(d.px(15.0)).color(t.accent));
                ui.label(
                    RichText::new("正在扫描设计库")
                        .font(Font::strong(d))
                        .color(t.text_primary),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(clock(state.clock.elapsed()))
                            .font(Font::mono_meta(d))
                            .color(t.text_muted),
                    );
                });
            });
            ui.add_space(d.px(6.0));
            ui.label(
                RichText::new(
                    "逐库比对会话号，单库可达数分钟。扫描期间不写入任何东西，超时重试即可。",
                )
                .font(Font::meta(d))
                .color(t.text_secondary),
            );
        });

        if let Some(cached) = &state.cache {
            let preview = &cached.preview;
            let totals = preview.totals();
            card(ui, t, d, |ui| {
                ui.horizontal(|ui| {
                    icon_label(
                        ui,
                        t,
                        d,
                        ph::CLOCK_COUNTER_CLOCKWISE,
                        t.text_muted,
                        "上次预览结果（已过期）",
                        t.text_secondary,
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(ago(cached.at.elapsed()))
                                .font(Font::mono_meta(d))
                                .color(t.text_muted),
                        );
                    });
                });
                ui.add_space(d.px(6.0));
                ui.label(
                    RichText::new(format!(
                        "{} 个数据批次 · {} 个交付单元 · {} 个待重试单元",
                        totals.batches,
                        totals.units,
                        preview.pending_model_retries.len()
                    ))
                    .font(Font::mono_meta(d))
                    .color(t.text_muted),
                );
            });
        }
    });
}

fn up_to_date_body(ui: &mut Ui, t: &Tokens, d: Density, state: &State) {
    let since = state
        .cache
        .as_ref()
        .map(|c| format!("上次扫描于 {}", ago(c.at.elapsed())))
        .unwrap_or_default();
    pad(ui, d, |ui| {
        note_card(
            ui,
            t,
            d,
            Status::Success,
            ph::CHECK_CIRCLE,
            "已是最新，没有可更新的内容",
            &format!("没有待应用的会话，也没有待重试的交付单元。这是正常结果，不是错误。{since}"),
        );
    });
}

/// S2 预览结果：左边变化树，右边 360 宽的汇总列。
fn preview_body(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    applying: usize,
    preview: &Preview,
    state: &mut State,
) {
    let totals = preview.totals();
    let rail_w = d.px(360.0);
    let full = ui.available_rect_before_wrap();
    let split = full.right() - rail_w;

    // 左：工具行 + 变化树（§2-G：SITE 平铺三段式——可执行 / 首次导入 / 本期不执行）
    let mut left = child(
        ui,
        Rect::from_min_max(full.left_top(), pos2(split, full.bottom())),
        Align::Min,
    );
    left.set_min_size(vec2(split - full.left(), full.height()));
    left.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        tree_toolbar(ui, t, d, preview, &totals, state);
        ScrollArea::vertical()
            .id_salt("model-update-change-tree")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(d.px(2.0));
                // 段一：可执行（SITE 平铺顶层）。非 DESI 完全不展示（2026-08-07
                // 验收定案），已是最新的库契约本就不返回。
                for db in &preview.dbnums {
                    if db.will_run() && !db.initialization_required {
                        executable_rows(ui, t, d, db, state);
                    }
                }
                // 段一末尾：首次导入库行（无段头，跟在 SITE 行之后——「不看文件」
                // 的唯一例外，文件名是它仅有的身份）。
                for db in &preview.dbnums {
                    if db.will_run() && db.initialization_required {
                        init_row(ui, t, d, db, state);
                    }
                }
                // 段二：本期不执行（分隔行）——阻断行与够不着行，无勾选框，
                // 行形态照 S2-E / S2-F 不变。
                let parked: Vec<&DbPreview> = preview
                    .dbnums
                    .iter()
                    .filter(|db| db.form().blocks() || db.form() == DbForm::NotInProject)
                    .collect();
                if !parked.is_empty() {
                    section_sep(ui, t, d, "本期不执行");
                    for db in parked {
                        parked_row(ui, t, d, db);
                    }
                }
                ui.add_space(d.px(8.0));
            });
    });

    // 右：本次范围 + 阻断与提示
    let rail = Rect::from_min_max(pos2(split, full.top()), full.right_bottom());
    ui.painter()
        .rect_filled(rail, CornerRadius::ZERO, t.bg_header);
    ui.painter().rect_filled(
        Rect::from_min_size(rail.left_top(), vec2(1.0, rail.height())),
        CornerRadius::ZERO,
        t.border,
    );
    let mut ui = child(ui, rail.shrink(d.px(16.0)), Align::Min);
    ui.set_min_size(rail.size() - Vec2::splat(d.px(32.0)));
    ScrollArea::vertical()
        .id_salt("model-update-summary-rail")
        .auto_shrink([false, false])
        .show(&mut ui, |ui| {
            summary_block(ui, t, d, applying, &totals);
            ui.add_space(d.px(20.0));
            hints_block(ui, t, d, preview, &totals);
        });
}

fn tree_toolbar(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    preview: &Preview,
    totals: &Totals,
    state: &mut State,
) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), d.px(38.0)), Sense::hover());
    let mut ui = child(ui, rect.shrink2(vec2(d.px(16.0), 0.0)), Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(8.0);
    ui.label(
        RichText::new("待更新范围")
            .font(Font::strong(d))
            .color(t.text_primary),
    );
    // 范围行只交代 DESI 与「够不着」（2026-08-07 验收定案：非 DESI 的数据完全
    // 不展示，连排除计数也不再交代）。「够不着」非 0 时才画。
    let mut scope = format!("{} 个 DESI 设计库在范围内", totals.desi);
    if totals.not_in_project > 0 {
        scope.push_str(&format!(
            " · {} 个 MDB 声明但项目内无文件",
            totals.not_in_project
        ));
    }
    ui.label(
        RichText::new(scope)
            .font(Font::mono_meta(d))
            .color(t.text_muted),
    );
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if ui
            .add(widgets::chip(
                t,
                d,
                "仅看影响模型",
                state.only_model_affecting,
            ))
            .on_hover_text("只列出会触发模型重新生成的交付单元")
            .clicked()
        {
            state.only_model_affecting = !state.only_model_affecting;
        }
        // 已勾选计数（§2-G 工具行）：人对着 SITE 做选择，机器按批次执行，
        // 桥在这里搭——两个数都给。
        let sel = selection(preview, state);
        ui.label(
            RichText::new(format!(
                "已勾选 {} / {} 个 SITE · 折算 {} 个数据批次",
                sel.site_checked, sel.site_total, sel.batches
            ))
            .font(Font::mono_meta(d))
            .color(t.text_muted),
        );
    });
}

/// 段间分隔行（§2-G「本期不执行」）。
fn section_sep(ui: &mut Ui, t: &Tokens, d: Density, title: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), d.px(28.0)), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let g = layout(ui, title, Font::meta(d));
    let x = rect.left() + d.px(12.0);
    ui.painter().galley(
        pos2(x, rect.center().y - g.size().y / 2.0),
        g.clone(),
        t.text_muted,
    );
    let line_l = x + g.size().x + d.px(10.0);
    ui.painter().rect_filled(
        Rect::from_min_max(
            pos2(line_l, rect.center().y),
            pos2(rect.right() - d.px(12.0), rect.center().y + 1.0),
        ),
        CornerRadius::ZERO,
        t.border,
    );
}

/// 可执行库（§2-G 段一）：SITE 平铺顶层。每个有名有姓的 SITE 桶一行顶层行，
/// 库级事实（时间对、sesno 区间、同库联动）在展开的详情行里按 SITE 重复画；
/// 归属未知桶排同库 SITE 行之后、无勾选框；纯位姿目标单独成组。
/// 老服务端没给 `sites[]`（或全部解不出 SITE 祖先）时退回文件身份行——
/// 会跑的批次绝不能因为没有 SITE 可说就从树上消失。
fn executable_rows(ui: &mut Ui, t: &Tokens, d: Density, db: &DbPreview, state: &mut State) {
    let checked = state.db_checked(db.dbnum);
    if db.named_sites().next().is_none() {
        legacy_db_rows(ui, t, d, db, state);
        return;
    }

    for site in &db.sites {
        if site.unknown() {
            continue;
        }
        let key = format!("db{}-site-{}", db.dbnum, site.site_refno);
        // 勾中的 SITE 默认摊开（用户打开这个窗就是来看它们的），取消勾选的
        // 默认收起——时间与区间是展开后的信息（原始诉求原文如此）。
        let open = state.open_row(&key, checked);
        let label = format!("SITE {}", site.display_name());
        let hit = row(
            ui,
            t,
            d,
            Row {
                check: Some(checked),
                caret: Some(open),
                icon: ph::MAP_PIN,
                icon_color: if checked { t.accent } else { t.text_muted },
                label: &label,
                label_color: if checked {
                    t.text_primary
                } else {
                    t.text_muted
                },
                strong: true,
                counts: Some((site.added, site.modified, site.deleted)),
                fill: t.bg_header,
                ..Row::default()
            },
        );
        if hit.check_clicked {
            // 同库联动（Q1 定案）：勾任何一个 SITE = 同库 SITE 全部跟着变。
            state.toggle_db(db.dbnum);
        } else if hit.clicked() {
            state.toggle_row(&key);
        }
        if !open {
            continue;
        }
        detail_row(ui, t, d, 1, &db.site_detail_line(site.display_name()));
        for unit in site
            .units
            .iter()
            .filter(|u| !state.only_model_affecting || u.will_generate)
        {
            unit_row(ui, t, d, 1, unit);
        }
    }

    for site in &db.sites {
        if !site.unknown() {
            continue;
        }
        let key = format!("db{}-site-unknown", db.dbnum);
        let open = state.open_row(&key, false);
        let hit = row(
            ui,
            t,
            d,
            Row {
                caret: Some(open),
                icon: ph::MAP_PIN,
                icon_color: t.text_muted,
                label: "SITE 归属未知",
                label_color: t.text_muted,
                strong: true,
                counts: Some((site.added, site.modified, site.deleted)),
                note: "解析不出 SITE 祖先 · 随本库批次执行",
                note_color: t.text_muted,
                fill: t.bg_header,
                ..Row::default()
            },
        );
        if hit.clicked() {
            state.toggle_row(&key);
        }
        if open {
            for unit in site
                .units
                .iter()
                .filter(|u| !state.only_model_affecting || u.will_generate)
            {
                unit_row(ui, t, d, 1, unit);
            }
        }
    }

    transform_rows(ui, t, d, db, state);
}

/// 老服务端（无 `sites[]`）的文件身份行：旧 S2 形态 + 勾选框，单元与位姿目标
/// 照旧挂在库行下。
fn legacy_db_rows(ui: &mut Ui, t: &Tokens, d: Density, db: &DbPreview, state: &mut State) {
    let form = db.form();
    let (tone_fg, _) = t.status(form.tone());
    let checked = state.db_checked(db.dbnum);
    let key = format!("db{}", db.dbnum);
    let expandable = !db.units.is_empty();
    let open = expandable && state.open_row(&key, checked);
    let head_label = if db.file_name.is_empty() {
        format!("db{} · {}", db.dbnum, db.db_type)
    } else {
        format!("{} · db{} · {}", db.file_name, db.dbnum, db.db_type)
    };
    let hit = row(
        ui,
        t,
        d,
        Row {
            check: Some(checked),
            caret: expandable.then_some(open),
            icon: form.icon(),
            icon_color: tone_fg,
            label: &head_label,
            label_color: if checked {
                t.text_primary
            } else {
                t.text_muted
            },
            strong: true,
            tag: form.label(),
            tag_color: tone_fg,
            meta: db.window(),
            note: form.note(),
            note_color: t.text_muted,
            fill: t.bg_header,
            ..Row::default()
        },
    );
    if hit.check_clicked {
        state.toggle_db(db.dbnum);
    } else if hit.clicked() && expandable {
        state.toggle_row(&key);
    }
    if !open {
        return;
    }
    for unit in db
        .units
        .iter()
        .filter(|u| !state.only_model_affecting || u.will_generate)
    {
        unit_row(ui, t, d, 1, unit);
    }
    transform_rows(ui, t, d, db, state);
}

/// 首次导入库行（§2-G）：有勾选框（会执行、可排除），无 SITE 行可给——
/// 「不看文件」的唯一例外。绝不能显示成「无变化」（S2-E 铁律不变）。
fn init_row(ui: &mut Ui, t: &Tokens, d: Density, db: &DbPreview, state: &mut State) {
    let checked = state.db_checked(db.dbnum);
    let (tone_fg, _) = t.status(Status::Warn);
    let label = if db.file_name.is_empty() {
        format!("db{} · {}", db.dbnum, db.db_type)
    } else {
        format!("{} · db{} · {}", db.file_name, db.dbnum, db.db_type)
    };
    let hit = row(
        ui,
        t,
        d,
        Row {
            check: Some(checked),
            icon: ph::SEAL_WARNING,
            icon_color: tone_fg,
            label: &label,
            label_color: if checked {
                t.text_primary
            } else {
                t.text_muted
            },
            strong: true,
            tag: "需初始化",
            tag_color: tone_fg,
            meta: "从未应用".into(),
            note: "首次导入 · 整库生成 · 会执行",
            note_color: tone_fg,
            fill: t.bg_header,
            ..Row::default()
        },
    );
    if hit.check_clicked {
        state.toggle_db(db.dbnum);
    }
}

/// 本期不执行的行（阻断 / 够不着）：无勾选框，行形态照 S2-E / S2-F 不变。
/// 这两类也是文件级身份——没有 SITE 可说时才许露文件名。
fn parked_row(ui: &mut Ui, t: &Tokens, d: Density, db: &DbPreview) {
    let form = db.form();
    let (tone_fg, tone_bg) = t.status(form.tone());
    let head_label = if db.file_name.is_empty() {
        format!("db{} · {}", db.dbnum, db.db_type)
    } else {
        format!("{} · db{} · {}", db.file_name, db.dbnum, db.db_type)
    };
    let _ = row(
        ui,
        t,
        d,
        Row {
            icon: form.icon(),
            icon_color: tone_fg,
            label: &head_label,
            label_color: if form.blocks() {
                t.danger
            } else {
                t.text_muted
            },
            strong: true,
            tag: form.label(),
            tag_color: tone_fg,
            // 够不着的库连文件都不在这个项目里，没有会话区间可说。
            meta: if form == DbForm::NotInProject {
                String::new()
            } else {
                db.window()
            },
            note: form.note(),
            note_color: if form.blocks() {
                t.danger
            } else {
                t.text_muted
            },
            fill: if form.blocks() { tone_bg } else { t.bg_header },
            ..Row::default()
        },
    );
}

/// SITE 展开首行：库级事实的详情行（时间对 · 待应用区间 · 同库联动名单）。
fn detail_row(ui: &mut Ui, t: &Tokens, d: Density, depth: usize, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), d.px(24.0)), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let x = rect.left() + d.px(10.0) + depth as f32 * d.px(16.0) + d.px(12.0) + d.px(6.0);
    glyph(
        ui,
        pos2(x + d.px(6.0), rect.center().y),
        ph::CLOCK,
        d.px(12.0),
        t.text_muted,
    );
    let g = layout(ui, text, Font::mono_meta(d));
    let clip = Rect::from_min_max(
        pos2(x + d.px(12.0) + d.px(6.0), rect.top()),
        pos2(rect.right() - d.px(12.0), rect.bottom()),
    );
    ui.painter().with_clip_rect(clip).galley(
        pos2(clip.left(), rect.center().y - g.size().y / 2.0),
        g,
        t.text_muted,
    );
}

fn unit_row(ui: &mut Ui, t: &Tokens, d: Density, depth: usize, unit: &UnitPreview) {
    let moved = match (unit.moved_in, unit.moved_out) {
        (0, 0) => String::new(),
        (i, 0) => format!("迁入 {i}"),
        (0, o) => format!("迁出 {o}"),
        (i, o) => format!("迁入 {i} · 迁出 {o}"),
    };
    let ulabel = format!("{} {}", unit.noun, unit.root_refno);
    let _ = row(
        ui,
        t,
        d,
        Row {
            depth,
            icon: crate::workbench::noun_icon(&unit.noun),
            icon_color: if unit.will_generate {
                t.warn
            } else {
                t.text_muted
            },
            label: &ulabel,
            label_color: t.text_primary,
            tag: unit.name.as_str(),
            tag_color: t.text_muted,
            meta: moved,
            note: if unit.will_generate {
                "待生成"
            } else {
                "不生成"
            },
            note_color: if unit.will_generate {
                t.warn
            } else {
                t.text_muted
            },
            ..Row::default()
        },
    );
}

/// 纯位姿目标单独成组：它们是「变换刷新」不是「重新生成」，混进交付单元里
/// 会让人以为要重建网格。容器目标（ZONE/SITE）额外说明整棵子树都会跟着动。
/// 契约不给它们的 SITE 归属，成组挂在本库 SITE 行之后、随本库批次勾选。
fn transform_rows(ui: &mut Ui, t: &Tokens, d: Density, db: &DbPreview, state: &mut State) {
    if db.transform_targets.is_empty() {
        return;
    }
    let key = format!("db{}-transform", db.dbnum);
    let open = state.open_row(&key, false);
    let hit = row(
        ui,
        t,
        d,
        Row {
            caret: Some(open),
            icon: ph::ARROWS_CLOCKWISE,
            icon_color: t.accent,
            label: "位姿刷新目标",
            label_color: t.text_primary,
            strong: true,
            meta: format!("{} 个", db.transform_targets.len()),
            note: "变换刷新 · 不重建网格 · 随本库批次执行",
            note_color: t.accent,
            fill: t.bg_header,
            ..Row::default()
        },
    );
    if hit.clicked() {
        state.toggle_row(&key);
    }
    if !open {
        return;
    }
    for target in &db.transform_targets {
        let tlabel = format!("{} {}", target.noun, target.refno);
        let _ = row(
            ui,
            t,
            d,
            Row {
                depth: 1,
                icon: crate::workbench::noun_icon(&target.noun),
                icon_color: t.accent,
                label: &tlabel,
                label_color: t.text_primary,
                tag: target.name.as_str(),
                tag_color: t.text_muted,
                meta: if target.container {
                    "整棵子树".to_owned()
                } else {
                    String::new()
                },
                note: if target.container {
                    "位姿刷新 · 子树"
                } else {
                    "位姿刷新"
                },
                note_color: t.accent,
                ..Row::default()
            },
        );
    }
}

fn summary_block(ui: &mut Ui, t: &Tokens, d: Density, applying: usize, totals: &Totals) {
    section(ui, t, d, "本次范围");
    ui.add_space(d.px(10.0));
    ui.horizontal(|ui| {
        let w = ui.available_width();
        let each = (w - d.px(2.0) * 2.0) / 3.0;
        for (i, (value, label, color)) in [
            (totals.added, "新增", t.success),
            (totals.modified, "修改", t.warn),
            (totals.deleted, "删除", t.danger),
        ]
        .into_iter()
        .enumerate()
        {
            if i > 0 {
                let (line, _) = ui.allocate_exact_size(vec2(1.0, d.px(30.0)), Sense::hover());
                ui.painter().rect_filled(line, CornerRadius::ZERO, t.border);
            }
            ui.allocate_ui(vec2(each - d.px(8.0), d.px(44.0)), |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(
                        RichText::new(value.to_string())
                            .font(FontId::new(d.px(22.0), FontFamily::Monospace))
                            .color(color),
                    );
                    ui.label(RichText::new(label).font(Font::meta(d)).color(t.text_muted));
                });
            });
        }
    });
    ui.add_space(d.px(10.0));
    ratio_bar(
        ui,
        t,
        d,
        &[
            (totals.added, t.success),
            (totals.modified, t.warn),
            (totals.deleted, t.danger),
        ],
    );
    ui.add_space(d.px(10.0));
    stat_row(
        ui,
        t,
        d,
        ph::STACK,
        t.accent,
        "受影响交付单元",
        &totals.units.to_string(),
        true,
    );
    ui.add_space(d.px(8.0));
    stat_row(
        ui,
        t,
        d,
        ph::SHAPES,
        t.text_muted,
        "其中影响模型的变化",
        &totals.model_affecting.to_string(),
        false,
    );
    if totals.transform_targets > 0 {
        ui.add_space(d.px(8.0));
        stat_row(
            ui,
            t,
            d,
            ph::ARROWS_CLOCKWISE,
            t.accent,
            "位姿刷新目标（不重建网格）",
            &totals.transform_targets.to_string(),
            false,
        );
    }
    // 预览与数据批次并发（合流后没有单飞预检）：正在被应用的会话也会被算进
    // 「待应用」。数字说不清来路就把来路说出来——为 0 时整条不画，不摆空话。
    if applying > 0 {
        ui.add_space(d.px(10.0));
        strip(
            ui,
            t,
            d,
            Status::Warn,
            ph::ARROWS_CLOCKWISE,
            &format!("{applying} 个库正在应用，以上数字可能偏大；应用完成后重新预览即为准确值"),
        );
    }
}

fn anomaly_copy(anomaly: &FileAnomaly) -> (&'static str, String) {
    match anomaly {
        FileAnomaly::Rollback {
            file_latest_sesno,
            applied_sesno,
        } => (
            "会话号回退，本次跳过",
            format!(
                "原因：文件最新会话号 {} 低于已应用 {}。\n影响：水位不回退，其他库照常执行。\n出路：换回正确文件。",
                group_number((*file_latest_sesno).into()),
                group_number((*applied_sesno).into())
            ),
        ),
        FileAnomaly::PathMigrated { old_path, new_path } => (
            "路径迁移，照常执行",
            format!(
                "原因：同号同类型且水位未变，文件从 {old_path} 移到 {new_path}。\n影响：本批次照常执行。\n出路：登记路径自动更新。"
            ),
        ),
        FileAnomaly::TypeChanged {
            stored_db_type,
            observed_db_type,
        } => (
            "库类型变化，本次跳过",
            format!(
                "原因：登记为 {stored_db_type}，本次读到 {observed_db_type}。\n影响：身份不自动覆盖，水位不变。\n出路：人工确认正确库类型。"
            ),
        ),
        FileAnomaly::Duplicate { paths } => (
            "发现同号文件，本次跳过",
            format!(
                "原因：项目内发现 {} 个同号文件：\n· {}\n影响：不自动挑选，水位不变。\n出路：人工保留正确文件。",
                paths.len(),
                paths.join("\n· ")
            ),
        ),
        FileAnomaly::Missing { path } => (
            "登记文件缺失，本次跳过",
            format!(
                "原因：登记路径找不到文件：{path}\n影响：本库不执行，水位不变。\n出路：放回文件或注销登记。"
            ),
        ),
    }
}

fn hints_block(ui: &mut Ui, t: &Tokens, d: Density, preview: &Preview, totals: &Totals) {
    section(ui, t, d, "阻断与提示");
    ui.add_space(d.px(10.0));
    let mut any = false;

    for db in preview.dbnums.iter().filter(|db| db.form().blocks()) {
        any = true;
        let (title, body) = db.anomaly.as_ref().map(anomaly_copy).unwrap_or((
            "文件异常，本次跳过",
            "影响：水位不变。\n出路：检查项目文件。".into(),
        ));
        note_card(
            ui,
            t,
            d,
            Status::Error,
            ph::WARNING_CIRCLE,
            &format!("db{} · {title}", db.dbnum),
            &body,
        );
        ui.add_space(d.px(10.0));
    }

    for db in preview
        .dbnums
        .iter()
        .filter(|db| db.form() == DbForm::PathMigrated)
    {
        any = true;
        let (title, body) = db
            .anomaly
            .as_ref()
            .map(anomaly_copy)
            .unwrap_or(("路径迁移，照常执行", String::new()));
        note_card(
            ui,
            t,
            d,
            Status::Warn,
            ph::GIT_MERGE,
            &format!("db{} · {title}", db.dbnum),
            &body,
        );
        ui.add_space(d.px(10.0));
    }

    if totals.transform_targets > 0 {
        any = true;
        note_card(
            ui,
            t,
            d,
            Status::Info,
            ph::ARROWS_CLOCKWISE,
            &format!("{} 个纯位姿目标走便宜路径刷新", totals.transform_targets),
            "只更新世界变换、包围盒、空间树与房间归属，不重建网格；ZONE/SITE 容器目标会连带刷新整棵子树的模型实例。",
        );
        ui.add_space(d.px(10.0));
    }

    if totals.no_generation > 0 {
        any = true;
        note_card(
            ui,
            t,
            d,
            Status::Warn,
            ph::WARNING,
            &format!("{} 项变化找不到可生成的交付单元", totals.no_generation),
            "已解析的最小交付单元照常生成，这些跳过并告警（no_generation）。",
        );
        ui.add_space(d.px(10.0));
    }

    fn listed<'a>(units: impl Iterator<Item = &'a PendingModelUnit>) -> String {
        units
            .map(|u| format!("{} {}（已尝试 {} 次）", u.noun, u.root_refno, u.attempts))
            .collect::<Vec<_>>()
            .join("；")
    }

    let to_run = preview.retries_to_run().count();
    let dead = preview.dead_retries().count();
    if to_run + dead > 0 {
        any = true;
        let mut detail = Vec::new();
        if to_run > 0 {
            detail.push(format!(
                "将合并：{}。按最新状态只生成一次。",
                listed(preview.retries_to_run())
            ));
        }
        if dead > 0 {
            detail.push(format!(
                "已放弃：{}。不再自动重试，也不并入手动更新；可在任务队列逐个重试。",
                listed(preview.dead_retries())
            ));
        }
        note_card(
            ui,
            t,
            d,
            Status::Info,
            ph::ARROW_COUNTER_CLOCKWISE,
            &format!("{to_run} 个将合并 · {dead} 个已放弃"),
            &detail.join("\n"),
        );
        ui.add_space(d.px(10.0));
    }

    for warning in &preview.warnings {
        any = true;
        note_card(ui, t, d, Status::Warn, ph::WARNING, "预览期告警", warning);
        ui.add_space(d.px(10.0));
    }

    if !any {
        ui.label(
            RichText::new("没有阻断，也没有需要交代的例外。")
                .font(Font::meta(d))
                .color(t.text_muted),
        );
    }
}

// ================================================================ 第二步：确认执行

/// S2-B。它不发任何请求，存在的唯一理由是**执行范围不等于预览列表**：
/// 阻断的库不跑、非 DESI 压根不在范围内、需初始化的库会跑、上次失败的单元会并入。
fn confirm_body(ui: &mut Ui, t: &Tokens, d: Density, preview: &Preview, state: &State) {
    let totals = preview.totals();
    let sel = selection(preview, state);
    let previewed_at = state
        .cache
        .as_ref()
        .map(|c| format!("预览完成于 {}；", ago(c.at.elapsed())))
        .unwrap_or_default();

    scroll_pad(ui, d, "model-update-confirm", |ui| {
        ui.spacing_mut().item_spacing.y = d.px(12.0);

        card_titled(
            ui,
            t,
            d,
            ph::PLAY,
            t.accent,
            "本次会执行",
            &format!("{} 个数据批次 · {} 个交付单元", sel.batches, sel.units),
            |ui| {
                // S2-H：批次行以同库 SITE 名单为题，dbnum 只在括号里露面
                // （任务队列 S12 按 dbnum 键，去那儿对账要认得出这一批是谁）。
                for db in preview
                    .dbnums
                    .iter()
                    .filter(|db| db.will_run() && state.db_checked(db.dbnum))
                {
                    if db.initialization_required {
                        line(
                            ui,
                            t,
                            d,
                            Dot::Tone(t.accent),
                            &format!("db{} · {}", db.dbnum, db.db_type),
                            &db.window(),
                            "需初始化 · 整库生成 · 建立水位",
                            t.text_secondary,
                        );
                        continue;
                    }
                    let sites = db.named_sites().count();
                    let note = if sites > 1 {
                        format!(
                            "同库一批（db{}）· {} 个会话 · {} 项变化",
                            db.dbnum,
                            db.sessions.len(),
                            db.changes()
                        )
                    } else {
                        format!(
                            "db{} · {} 个会话 · {} 项变化",
                            db.dbnum,
                            db.sessions.len(),
                            db.changes()
                        )
                    };
                    line(
                        ui,
                        t,
                        d,
                        Dot::Tone(t.accent),
                        &db.batch_title(),
                        &db.window(),
                        &note,
                        t.text_secondary,
                    );
                }
                line(
                    ui,
                    t,
                    d,
                    Dot::Glyph(ph::STACK, t.accent),
                    &format!("{} 个最小交付单元将重新生成", sel.generating),
                    "",
                    "按变化归并；初始化库的整库生成单元不计入此数",
                    t.text_muted,
                );
                if sel.transform_targets > 0 {
                    line(
                        ui,
                        t,
                        d,
                        Dot::Glyph(ph::ARROWS_CLOCKWISE, t.accent),
                        &format!("{} 个位姿目标只做变换刷新", sel.transform_targets),
                        "",
                        "不重建网格；容器目标连带整棵子树",
                        t.text_muted,
                    );
                }
                ui.add_space(d.px(6.0));
                // 重扫提示限定勾选范围（ADR-020：过滤作用于重扫循环）。
                strip(
                    ui,
                    t,
                    d,
                    Status::Info,
                    ph::GIT_MERGE,
                    &format!(
                        "{previewed_at}开始时对勾选范围重新扫描，新会话自动并入本批次并逐条列出；未勾选的不扫描、不入队"
                    ),
                );
            },
        );

        let dead = preview.dead_retries().count();
        let unselected: Vec<&DbPreview> = preview
            .dbnums
            .iter()
            .filter(|db| db.will_run() && !state.db_checked(db.dbnum))
            .collect();
        // 非 DESI 不占行也不进计数（2026-08-07 验收定案：排除行已删）。
        let not_running = unselected.len()
            + totals.blocked
            + totals.not_in_project
            + usize::from(totals.no_generation > 0);
        card_titled(
            ui,
            t,
            d,
            ph::PROHIBIT,
            t.text_muted,
            "本次不会执行",
            &format!("{not_running} 项"),
            |ui| {
                // 未勾选行：人自己选的，出路就是回上一步勾上。与阻断、够不着
                // 不是一回事，三类不许合成一行。
                for db in &unselected {
                    line(
                        ui,
                        t,
                        d,
                        Dot::Glyph(ph::SQUARE, t.text_muted),
                        &db.batch_title(),
                        &format!("{} · {} 个会话待应用", db.window(), db.sessions.len()),
                        "未勾选 · 本次跳过，水位不变",
                        t.text_muted,
                    );
                }
                for db in preview.dbnums.iter().filter(|db| db.form().blocks()) {
                    line(
                        ui,
                        t,
                        d,
                        Dot::Tone(t.danger),
                        &format!("db{} · {}", db.dbnum, db.db_type),
                        db.form().label(),
                        "已阻断 · 水位不变",
                        t.danger,
                    );
                }
                for db in preview
                    .dbnums
                    .iter()
                    .filter(|db| db.form() == DbForm::NotInProject)
                {
                    line(
                        ui,
                        t,
                        d,
                        Dot::Tone(t.text_muted),
                        &format!("db{} · {}", db.dbnum, db.db_type),
                        "MDB 声明了它，当前项目目录里没有这个文件",
                        "本期扫描够不着",
                        t.text_muted,
                    );
                }
                if totals.no_generation > 0 {
                    line(
                        ui,
                        t,
                        d,
                        Dot::Glyph(ph::WARNING, t.warn),
                        &format!("{} 项变化找不到可生成的交付单元", totals.no_generation),
                        "",
                        "计入 no_generation 并告警",
                        t.warn,
                    );
                }
                if not_running == 0 {
                    ui.label(
                        RichText::new("没有未勾选、阻断或够不着的库。")
                            .font(Font::meta(d))
                            .color(t.text_muted),
                    );
                }
            },
        );

        let to_run = preview.retries_to_run().count();
        if to_run + dead > 0 {
            card_titled(
                ui,
                t,
                d,
                ph::ARROW_COUNTER_CLOCKWISE,
                t.accent,
                "会一并处理",
                &format!("{to_run} 个将合并 · {dead} 个已放弃"),
                |ui| {
                    for unit in preview.retries_to_run() {
                        line(
                            ui,
                            t,
                            d,
                            Dot::Tone(t.accent),
                            &format!("{} {}", unit.noun, unit.root_refno),
                            &format!(
                                "上次生成失败 · 已尝试 {} 次 · 来源会话 {}",
                                unit.attempts, unit.source_end_sesno
                            ),
                            "按最新状态只生成一次",
                            t.text_secondary,
                        );
                    }
                    for unit in preview.dead_retries() {
                        line(
                            ui,
                            t,
                            d,
                            Dot::Tone(t.danger),
                            &format!("{} {}", unit.noun, unit.root_refno),
                            &format!("已尝试 {} 次", unit.attempts),
                            "已放弃 · 本次不执行 · 可在任务队列立刻重试",
                            t.danger,
                        );
                    }
                },
            );
        }

        note_card(
            ui,
            t,
            d,
            Status::Warn,
            ph::WARNING,
            "开始后不能中途取消",
            "数据批次一旦成功，已应用会话号立即推进且不回滚；模型生成失败的单元转入待重试，不影响已写入的数据。关闭窗口不会停任务。",
        );
    });
}

// ================================================================ 失败态

/// S2-D。按 `code` 分型给出「原因 / 影响 / 一个出路」；
/// **只有 internal 那一类**才把原始 message 摊出来，且收进「详情」默认折起。
fn failure_body(ui: &mut Ui, t: &Tokens, d: Density, failure: &Failure, state: &mut State) {
    let form = failure.form();
    pad(ui, d, |ui| {
        ui.spacing_mut().item_spacing.y = d.px(12.0);
        note_card(
            ui,
            t,
            d,
            form.tone(),
            form.icon(),
            form.title(),
            form.body(),
        );

        if form == FailForm::Internal {
            let label = if state.detail_open {
                "收起详情"
            } else {
                "详情"
            };
            if ui
                .add(widgets::chip(t, d, label, state.detail_open))
                .clicked()
            {
                state.detail_open = !state.detail_open;
            }
            if state.detail_open {
                ui.add_space(d.px(8.0));
                card(ui, t, d, |ui| {
                    ui.label(
                        RichText::new(&failure.message)
                            .font(Font::mono_meta(d))
                            .color(t.text_secondary),
                    );
                    if let Some(detail) = failure.detail.as_deref() {
                        ui.add_space(d.px(6.0));
                        ui.label(
                            RichText::new(detail)
                                .font(Font::mono_micro(d))
                                .color(t.text_muted),
                        );
                    }
                });
            }
        } else if matches!(form, FailForm::IdentityMismatch | FailForm::WorkerDisabled)
            && !failure.message.is_empty()
        {
            // 服务端的 message 会点名是 project / mdb / namespace 里哪一项错开
            // （本地闸门合成的那条则说明身份尚未就绪 / 服务为何不消化），
            // 它是人接下来唯一用得上的东西。
            ui.label(
                RichText::new(&failure.message)
                    .font(Font::mono_meta(d))
                    .color(t.text_muted),
            );
        }
    });
}

// ================================================================ 零件

enum Corner {
    Top,
    Bottom,
    None,
}

/// 一条通栏横条。返回它的矩形，调用点在里面摆内容。
fn bar(ui: &mut Ui, h: f32, fill: Color32, corner: Corner) -> Rect {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let r = radius::LG;
    let cr = match corner {
        Corner::Top => CornerRadius {
            nw: r,
            ne: r,
            sw: 0,
            se: 0,
        },
        Corner::Bottom => CornerRadius {
            nw: 0,
            ne: 0,
            sw: r,
            se: r,
        },
        Corner::None => CornerRadius::ZERO,
    };
    ui.painter().rect_filled(rect, cr, fill);
    rect
}

fn hairline(painter: &egui::Painter, rect: Rect, color: Color32) {
    painter.rect_filled(
        Rect::from_min_size(
            pos2(rect.left(), rect.bottom() - 1.0),
            vec2(rect.width(), 1.0),
        ),
        CornerRadius::ZERO,
        color,
    );
}

fn child(ui: &mut Ui, rect: Rect, align: Align) -> Ui {
    ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(align)),
    )
}

fn pad<R>(ui: &mut Ui, d: Density, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(Margin::same(d.px(16.0) as i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn scroll_pad(ui: &mut Ui, d: Density, salt: &str, add: impl FnOnce(&mut Ui)) {
    ScrollArea::vertical()
        .id_salt(salt)
        .auto_shrink([false, false])
        .show(ui, |ui| pad(ui, d, add));
}

fn center_note(ui: &mut Ui, t: &Tokens, d: Density, icon: Option<&str>, title: &str, detail: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() / 2.0 - d.px(56.0)).max(d.px(40.0)));
        ui.spacing_mut().item_spacing.y = d.px(8.0);
        match icon {
            Some(icon) => ui.label(
                RichText::new(icon)
                    .font(FontId::new(d.px(26.0), FontFamily::Proportional))
                    .color(t.text_muted),
            ),
            None => ui.add(egui::Spinner::new().size(d.px(26.0)).color(t.accent)),
        };
        ui.label(
            RichText::new(title)
                .font(Font::strong(d))
                .color(t.text_secondary),
        );
        ui.label(
            RichText::new(detail)
                .font(Font::meta(d))
                .color(t.text_muted),
        );
    });
}

fn card<R>(ui: &mut Ui, t: &Tokens, d: Density, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(t.bg_elevated)
        .stroke(Stroke::new(1.0_f32, t.border))
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(Margin::same(d.px(14.0) as i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui)
        })
        .inner
}

#[allow(clippy::too_many_arguments)]
fn card_titled(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    icon: &str,
    icon_color: Color32,
    title: &str,
    note: &str,
    add: impl FnOnce(&mut Ui),
) {
    card(ui, t, d, |ui| {
        ui.horizontal(|ui| {
            icon_label(ui, t, d, icon, icon_color, title, t.text_primary);
            if !note.is_empty() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(note)
                            .font(Font::mono_meta(d))
                            .color(t.text_secondary),
                    );
                });
            }
        });
        ui.add_space(d.px(8.0));
        ui.spacing_mut().item_spacing.y = 0.0;
        add(ui);
    });
}

fn icon_label(
    ui: &mut Ui,
    _t: &Tokens,
    d: Density,
    icon: &str,
    icon_color: Color32,
    text: &str,
    text_color: Color32,
) {
    ui.spacing_mut().item_spacing.x = d.px(8.0);
    let (rect, _) = ui.allocate_exact_size(vec2(d.px(15.0), d.px(15.0)), Sense::hover());
    glyph(ui, rect.center(), icon, d.px(14.0), icon_color);
    ui.label(RichText::new(text).font(Font::strong(d)).color(text_color));
}

fn note_card(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    tone: Status,
    icon: &str,
    title: &str,
    detail: &str,
) {
    let (fg, bg) = t.status(tone);
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(Margin::same(d.px(10.0) as i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = d.px(10.0);
                let (rect, _) =
                    ui.allocate_exact_size(vec2(d.px(14.0), d.px(18.0)), Sense::hover());
                glyph(ui, rect.center(), icon, d.px(14.0), fg);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = d.px(4.0);
                    ui.label(RichText::new(title).font(Font::strong(d)).color(fg));
                    if !detail.is_empty() {
                        ui.label(
                            RichText::new(detail)
                                .font(Font::meta(d))
                                .color(t.text_secondary),
                        );
                    }
                });
            });
        });
}

/// 卡片内的一条浅底提示。比 `note_card` 轻，用来解释上一行是怎么回事。
fn strip(ui: &mut Ui, t: &Tokens, d: Density, tone: Status, icon: &str, text: &str) {
    let (fg, bg) = t.status(tone);
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(radius::SM))
        .inner_margin(Margin::symmetric(d.px(10.0) as i8, d.px(7.0) as i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = d.px(8.0);
                let (rect, _) =
                    ui.allocate_exact_size(vec2(d.px(13.0), d.px(15.0)), Sense::hover());
                glyph(ui, rect.center(), icon, d.px(12.0), fg);
                ui.label(
                    RichText::new(text)
                        .font(Font::meta(d))
                        .color(t.text_secondary),
                );
            });
        });
}

fn section(ui: &mut Ui, t: &Tokens, d: Density, title: &str) {
    ui.label(
        RichText::new(title)
            .font(Font::strong(d))
            .color(t.text_muted),
    );
}

fn stat_row(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    icon: &str,
    icon_color: Color32,
    label: &str,
    value: &str,
    strong: bool,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = d.px(8.0);
        let (rect, _) = ui.allocate_exact_size(vec2(d.px(13.0), d.px(13.0)), Sense::hover());
        glyph(ui, rect.center(), icon, d.px(13.0), icon_color);
        ui.label(
            RichText::new(label)
                .font(Font::body(d))
                .color(t.text_secondary),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(value)
                    .font(if strong {
                        Font::mono(d)
                    } else {
                        Font::mono_meta(d)
                    })
                    .color(if strong {
                        t.text_primary
                    } else {
                        t.text_secondary
                    }),
            );
        });
    });
}

/// 按比例铺开的横条。分布条与进度条是同一个零件：都只是「几段各占多少」。
fn ratio_bar(ui: &mut Ui, t: &Tokens, d: Density, segments: &[(u32, Color32)]) {
    let h = d.px(8.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let cr = CornerRadius::same((h / 2.0) as u8);
    ui.painter().rect_filled(rect, cr, t.bg_input);
    let total: u32 = segments.iter().map(|(v, _)| *v).sum();
    if total == 0 {
        return;
    }
    let painter = ui.painter().with_clip_rect(rect);
    let mut x = rect.left();
    for (value, color) in segments {
        let w = rect.width() * (*value as f32) / (total as f32);
        if *color != Color32::TRANSPARENT && w > 0.0 {
            painter.rect_filled(
                Rect::from_min_size(pos2(x, rect.top()), vec2(w, h)),
                cr,
                *color,
            );
        }
        x += w;
    }
}

/// 行首那枚状态标记。
enum Dot {
    Tone(Color32),
    Glyph(&'static str, Color32),
}

/// 卡片里的一条明细行：标记 + 等宽标识 + 次要说明 + 右对齐的状态词。
fn line(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    dot: Dot,
    label: &str,
    detail: &str,
    note: &str,
    note_color: Color32,
) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), d.px(28.0)), Sense::hover());
    let mut ui = child(ui, rect, Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(8.0);
    let (mark, _) = ui.allocate_exact_size(vec2(d.px(14.0), d.px(14.0)), Sense::hover());
    match dot {
        Dot::Tone(color) => {
            ui.painter().circle_filled(mark.center(), d.px(4.0), color);
        }
        Dot::Glyph(icon, color) => glyph(&ui, mark.center(), icon, d.px(13.0), color),
    }
    ui.label(
        RichText::new(label)
            .font(Font::mono(d))
            .color(t.text_primary),
    );
    if !detail.is_empty() {
        ui.label(
            RichText::new(detail)
                .font(Font::mono_meta(d))
                .color(t.text_muted),
        );
    }
    if !note.is_empty() {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(note).font(Font::meta(d)).color(note_color));
        });
    }
}

/// 变化树的一行。设计稿上它比通用 `tree_row` 多一段「行尾状态词」，
/// 而状态词恰恰是这棵树最要紧的一列，所以单独画。
struct Row<'a> {
    depth: usize,
    /// `Some(勾中与否)` 画一枚勾选框（§2-G SITE 行 / 首次导入行）。点它走
    /// [`RowResponse::check_clicked`]，不与整行的展开点击混在一起。
    check: Option<bool>,
    caret: Option<bool>,
    icon: &'a str,
    icon_color: Color32,
    label: &'a str,
    label_color: Color32,
    strong: bool,
    tag: &'a str,
    tag_color: Color32,
    /// `Some((增, 改, 删))` 在 meta 左侧画三段等宽计数 `+a ~m −d`
    /// （success / accent-strong / danger，§2-G SITE 行）。零值段不画。
    counts: Option<(u32, u32, u32)>,
    meta: String,
    note: &'a str,
    note_color: Color32,
    fill: Color32,
}

impl Default for Row<'_> {
    fn default() -> Self {
        Row {
            depth: 0,
            check: None,
            caret: None,
            icon: "",
            icon_color: Color32::PLACEHOLDER,
            label: "",
            label_color: Color32::PLACEHOLDER,
            strong: false,
            tag: "",
            tag_color: Color32::PLACEHOLDER,
            counts: None,
            meta: String::new(),
            note: "",
            note_color: Color32::PLACEHOLDER,
            fill: Color32::TRANSPARENT,
        }
    }
}

struct RowResponse {
    resp: egui::Response,
    /// 勾选框被点了（这次点击不再算整行点击——两个动作互斥）。
    check_clicked: bool,
}

impl RowResponse {
    /// 整行点击（展开 / 收起），勾选框的点击已经被摘走。
    fn clicked(&self) -> bool {
        !self.check_clicked && self.resp.clicked()
    }
}

fn row(ui: &mut Ui, t: &Tokens, d: Density, r: Row<'_>) -> RowResponse {
    let h = d.px(30.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    if !ui.is_rect_visible(rect) {
        return RowResponse {
            resp,
            check_clicked: false,
        };
    }
    let bg = if r.fill != Color32::TRANSPARENT {
        r.fill
    } else if resp.hovered() {
        t.bg_hover
    } else {
        Color32::TRANSPARENT
    };
    if bg != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);
    }

    let gap = d.px(6.0);
    let mut x = rect.left() + d.px(10.0) + r.depth as f32 * d.px(16.0);
    let mut check_clicked = false;
    if let Some(checked) = r.check {
        let side = d.px(14.0);
        let check_rect =
            Rect::from_center_size(pos2(x + side / 2.0, rect.center().y), Vec2::splat(side));
        // 后注册的交互在命中测试里优先（与工具栏同一条规矩），点勾选框不会
        // 顺带触发整行的展开。
        let check_resp = ui.interact(
            check_rect.expand(d.px(3.0)),
            resp.id.with("check"),
            Sense::click(),
        );
        check_clicked = check_resp.clicked();
        if checked {
            ui.painter()
                .rect_filled(check_rect, CornerRadius::same(radius::SM), t.accent);
            glyph(ui, check_rect.center(), ph::CHECK, d.px(10.0), t.accent_ink);
        } else {
            ui.painter().rect_stroke(
                check_rect,
                CornerRadius::same(radius::SM),
                Stroke::new(1.0, t.border_strong),
                StrokeKind::Inside,
            );
        }
        if check_resp.hovered() {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
        }
        x += side + gap;
    }
    if let Some(open) = r.caret {
        let icon = if open {
            ph::CARET_DOWN
        } else {
            ph::CARET_RIGHT
        };
        glyph(
            ui,
            pos2(x + d.px(6.0), rect.center().y),
            icon,
            d.px(12.0),
            t.text_muted,
        );
    }
    x += d.px(12.0) + gap;
    glyph(
        ui,
        pos2(x + d.px(7.0), rect.center().y),
        r.icon,
        d.px(14.0),
        r.icon_color,
    );
    x += d.px(14.0) + gap;

    // 右侧先量后画：状态词与 meta 占了多少，决定了标识能写到哪。
    let mut right = rect.right() - d.px(12.0);
    if !r.note.is_empty() {
        let g = layout(ui, r.note, Font::meta(d));
        right -= g.size().x;
        ui.painter().galley(
            pos2(right, rect.center().y - g.size().y / 2.0),
            g,
            r.note_color,
        );
        right -= gap * 2.0;
    }
    if !r.meta.is_empty() {
        let g = layout(ui, &r.meta, Font::mono_meta(d));
        right -= g.size().x;
        ui.painter().galley(
            pos2(right, rect.center().y - g.size().y / 2.0),
            g,
            t.text_muted,
        );
        right -= gap * 2.0;
    }
    if let Some((added, modified, deleted)) = r.counts {
        // 从右往左画：−d、~m、+a，零值段不画（没有变化就别占地方）。
        for (value, prefix, color) in [
            (deleted, "−", t.danger),
            (modified, "~", t.accent_strong),
            (added, "+", t.success),
        ] {
            if value == 0 {
                continue;
            }
            let g = layout(ui, &format!("{prefix}{value}"), Font::mono_meta(d));
            right -= g.size().x;
            ui.painter()
                .galley(pos2(right, rect.center().y - g.size().y / 2.0), g, color);
            right -= gap;
        }
        right -= gap;
    }

    let font = if r.strong {
        Font::strong(d)
    } else {
        Font::body(d)
    };
    let g = layout(ui, r.label, font);
    let clip = Rect::from_min_max(pos2(x, rect.top()), pos2(right.max(x), rect.bottom()));
    let label_w = g.size().x;
    ui.painter().with_clip_rect(clip).galley(
        pos2(x, rect.center().y - g.size().y / 2.0),
        g,
        r.label_color,
    );
    if !r.tag.is_empty() {
        let tg = layout(ui, r.tag, Font::meta(d));
        let tx = x + label_w + gap * 2.0;
        ui.painter().with_clip_rect(clip).galley(
            pos2(tx, rect.center().y - tg.size().y / 2.0),
            tg,
            r.tag_color,
        );
    }
    RowResponse {
        resp,
        check_clicked,
    }
}

fn stopwatch(ui: &mut Ui, t: &Tokens, d: Density, label: &str, elapsed: Duration) {
    ui.label(
        RichText::new(format!("{label} {}", clock(elapsed)))
            .font(Font::mono_meta(d))
            .color(t.text_secondary),
    );
}

// ---------------------------------------------------------------- 绘制小工具

fn layout(ui: &Ui, text: &str, font: FontId) -> std::sync::Arc<egui::Galley> {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER)
}

fn glyph(ui: &Ui, center: egui::Pos2, text: &str, size: f32, color: Color32) {
    let g = layout(ui, text, FontId::new(size, FontFamily::Proportional));
    ui.painter().galley(center - g.size() / 2.0, g, color);
}

fn mono_at(ui: &Ui, center: egui::Pos2, text: &str, d: Density, color: Color32) {
    let g = layout(ui, text, Font::mono_meta(d));
    ui.painter().galley(center - g.size() / 2.0, g, color);
}

fn fill(ui: &Ui, rect: Rect, r: u8, bg: Color32, stroke: Option<Color32>) {
    ui.painter().rect_filled(rect, CornerRadius::same(r), bg);
    if let Some(s) = stroke {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(r),
            Stroke::new(1.0_f32, s),
            StrokeKind::Inside,
        );
    }
}

// ---------------------------------------------------------------- 数与时间

fn clock(elapsed: Duration) -> String {
    let s = elapsed.as_secs();
    format!("{:02}:{:02}", s / 60, s % 60)
}

fn ago(elapsed: Duration) -> String {
    let s = elapsed.as_secs();
    match s {
        0..=59 => format!("{s} 秒前"),
        60..=3599 => format!("{} 分钟前", s / 60),
        _ => format!("{} 小时前", s / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applied_data_triggers_refresh_even_when_every_model_unit_failed() {
        let outcome: Outcome = serde_json::from_str(
            r#"{
                "batch":{"dbnum":7997,"start_sesno":41,"end_sesno":42,
                    "status":"applied","changed_elements":3},
                "units":[{"dbnum":7997,"root_refno":"24384/12","noun":"EQUI",
                    "status":"failed","attempts":1}]
            }"#,
        )
        .unwrap();

        assert!(outcome.data_applied());
        assert!(outcome.refresh_units().is_empty());
    }

    fn db(dbnum: u32, db_type: &str) -> DbPreview {
        DbPreview {
            dbnum,
            db_type: db_type.into(),
            ..Default::default()
        }
    }

    /// 一个「够不着」的库，**带 `initialization_required`**。
    ///
    /// 这个标志是这条 fixture 的全部意义所在：够不着的库正是从没导入过的那些
    /// （`/ALL` 的 29 个里 10 个没导入，除去磁盘上有文件的 1112，剩下 9 个就是它们），
    /// 没有权威水位，契约给的就是 `initialization_required = true`。少了它，
    /// 一个默认 `DbPreview` 本来就 `!will_run()`——用它去断言「够不着的不跑」，
    /// 断言会通过，但通过的理由与 `not_in_project` 毫无关系。
    fn unreachable_db(dbnum: u32) -> DbPreview {
        DbPreview {
            not_in_project: true,
            initialization_required: true,
            ..db(dbnum, "DESI")
        }
    }

    /// S2-E 的七种行形态。最要紧的两条：`PathMigrated` 是五种异常里唯一不阻断的，
    /// `initialization_required` 的库 `net_*` 全是 0 却确实会跑。
    #[test]
    fn seven_row_forms_split_by_whether_the_batch_runs() {
        let mut pending = db(8000, "DESI");
        pending.sessions.push(SessionPreview::default());
        assert_eq!(pending.form(), DbForm::Pending);
        assert!(pending.will_run());

        let mut init = db(8021, "DESI");
        init.initialization_required = true;
        assert_eq!(init.form(), DbForm::Initialize);
        assert!(init.will_run(), "需初始化的库没有会话区间，但它确实会跑");
        assert_ne!(init.form(), DbForm::UpToDate);

        let mut migrated = db(8044, "DESI");
        migrated.sessions.push(SessionPreview::default());
        migrated.anomaly = Some(FileAnomaly::PathMigrated {
            old_path: "a".into(),
            new_path: "b".into(),
        });
        assert_eq!(migrated.form(), DbForm::PathMigrated);
        assert!(!migrated.form().blocks());
        assert!(migrated.will_run());

        for (dbnum, anomaly, want) in [
            (
                8003,
                FileAnomaly::Rollback {
                    file_latest_sesno: 812,
                    applied_sesno: 1005,
                },
                DbForm::Rollback,
            ),
            (
                8107,
                FileAnomaly::TypeChanged {
                    stored_db_type: "DESI".into(),
                    observed_db_type: "CATA".into(),
                },
                DbForm::TypeChanged,
            ),
            (
                8150,
                FileAnomaly::Duplicate {
                    paths: vec!["a".into(), "b".into()],
                },
                DbForm::Duplicate,
            ),
            (
                8163,
                FileAnomaly::Missing { path: "a".into() },
                DbForm::Missing,
            ),
        ] {
            let mut blocked = db(dbnum, "DESI");
            blocked.anomaly = Some(anomaly);
            blocked.blocked = true;
            assert_eq!(blocked.form(), want);
            assert!(want.blocks());
            assert!(!blocked.will_run());
        }

        let excluded = db(8191, "CATA");
        assert_eq!(excluded.form(), DbForm::Excluded);
        assert!(!excluded.will_run());
    }

    /// 「够不着」与「文件缺失」是两回事，最要紧的是**它不阻断**：MDB 声明了
    /// 一个库、当前项目目录里没有它的文件，这不是异常，只是本期扫描碰不到它。
    /// 合成一行的话，AvevaCatalogue 那 9 个模板库会顶着「已阻断」出现在界面上，
    /// 人会去找根本不存在的故障。
    #[test]
    fn declared_by_mdb_but_absent_from_the_project_is_not_a_missing_file() {
        let unreachable = unreachable_db(7015);

        assert_eq!(unreachable.form(), DbForm::NotInProject);
        assert!(!DbForm::NotInProject.blocks(), "够不着不是阻断");
        assert!(!unreachable.will_run());

        let mut missing = db(7016, "DESI");
        missing.anomaly = Some(FileAnomaly::Missing { path: "a".into() });
        missing.blocked = true;
        assert_ne!(missing.form(), unreachable.form());
    }

    /// 「够不着」的行每次预览都在，算进待办的话「已是最新」永远到不了。
    /// 它也不许并进 `desi`——那个数要跟 MDB 声明数一对才说得清差在哪儿。
    #[test]
    fn unreachable_rows_are_counted_apart_and_never_block_up_to_date() {
        let preview = Preview {
            dbnums: vec![db(8000, "DESI"), unreachable_db(7015), db(8191, "CATA")],
            ..Default::default()
        };

        let totals = preview.totals();
        assert_eq!(totals.desi, 1, "够不着的库不算在范围内的设计库里");
        assert_eq!(totals.not_in_project, 1);
        assert_eq!(totals.excluded, 1);
        assert_eq!(totals.batches, 0);
        assert!(!preview.executable());
    }

    /// 死信不算「将合并」，也撑不起一次运行。
    ///
    /// 执行侧按服务端的重试上限封顶，到顶的行一次都不会跑；照旧算进去的话，
    /// 「开始更新」在一个只剩死信的项目上会永远亮着，按下去扫一圈什么也没动，
    /// 而那几个根的模型一直停在旧几何。
    #[test]
    fn dead_letters_are_neither_merged_nor_enough_to_start_a_run() {
        let alive = PendingModelUnit {
            root_refno: "24381/1".into(),
            attempts: 2,
            ..Default::default()
        };
        let dead = PendingModelUnit {
            root_refno: "24381/2".into(),
            attempts: 5,
            dead: true,
            ..Default::default()
        };

        let both = Preview {
            pending_model_retries: vec![alive, dead.clone()],
            ..Default::default()
        };
        assert_eq!(both.retries_to_run().count(), 1);
        assert_eq!(both.dead_retries().count(), 1);
        assert!(both.executable(), "还有活着的待重试单元就该能跑");

        let only_dead = Preview {
            pending_model_retries: vec![dead],
            ..Default::default()
        };
        assert_eq!(only_dead.retries_to_run().count(), 0);
        assert!(
            !only_dead.executable(),
            "只剩死信时按钮不该亮：这一次它一个都不会碰"
        );
    }

    /// 契约里那面 `dead` 旗必须真解得出来。
    ///
    /// 它带 `serde(default)`（老服务端不给这个字段），所以服务端一旦改名，
    /// 客户端不会报错、只会静默把每一行都当成「还在自动重试」——正是这次要修的
    /// 那句假话。这里断的是「真解出来了」，不是「解得动」。
    #[test]
    fn the_dead_letter_flag_survives_deserialization() {
        let units: Vec<PendingModelUnit> = serde_json::from_str(
            r#"[{"dbnum":7997,"root_refno":"24381/2","noun":"BRAN",
                 "source_end_sesno":42,"attempts":5,"last_error":"boom","dead":true},
                {"dbnum":7997,"root_refno":"24381/3","noun":"EQUI",
                 "source_end_sesno":42,"attempts":1}]"#,
        )
        .expect("契约要解得出来");
        assert!(units[0].dead);
        assert_eq!(units[0].last_error.as_deref(), Some("boom"));
        assert!(!units[1].dead, "老 payload 缺字段时缺省不许标成已放弃");
    }

    /// 「排除」与「阻断」来自契约的不同位置，汇总里也得分开数。
    #[test]
    fn totals_count_batches_excluded_and_blocked_separately() {
        let mut pending = db(8000, "DESI");
        pending.sessions.push(SessionPreview::default());
        pending.net_added = 128;
        pending.net_modified = 342;
        pending.net_deleted = 17;
        pending.model_affecting = 361;
        pending.no_generation = 14;
        pending.units = vec![UnitPreview::default(); 23];
        pending.units[0].will_generate = true;
        pending.transform_targets = vec![TransformTargetPreview::default(); 2];

        let mut blocked = db(8003, "DESI");
        blocked.blocked = true;
        blocked.anomaly = Some(FileAnomaly::Rollback {
            file_latest_sesno: 812,
            applied_sesno: 1005,
        });

        let preview = Preview {
            project: "AMS-8009".into(),
            dbnums: vec![pending, blocked, db(8191, "CATA")],
            ..Default::default()
        };
        let totals = preview.totals();

        assert_eq!(
            (totals.added, totals.modified, totals.deleted),
            (128, 342, 17)
        );
        assert_eq!(totals.units, 23);
        assert_eq!(totals.generating, 1);
        assert_eq!(totals.transform_targets, 2);
        assert_eq!(totals.model_affecting, 361);
        assert_eq!(totals.no_generation, 14);
        assert_eq!(totals.batches, 1);
        assert_eq!(totals.desi, 2);
        assert_eq!(totals.excluded, 1);
        assert_eq!(totals.blocked, 1);
        assert!(preview.executable());
    }

    /// 待重试单元自己就够成一次运行——契约明写没有新 sesno 也要允许重试。
    #[test]
    fn pending_retries_alone_make_a_run_executable() {
        let preview = Preview {
            dbnums: vec![db(8191, "CATA")],
            pending_model_retries: vec![PendingModelUnit {
                dbnum: 8000,
                root_refno: "24381/1".into(),
                noun: "EQUI".into(),
                attempts: 2,
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(preview.totals().batches, 0);
        assert!(preview.executable());
    }

    /// 失败按 `code` 分型，不解析 message 字符串。
    #[test]
    fn failures_are_classified_by_code_only() {
        assert_eq!(
            Failure::new(
                "identity_mismatch",
                "请求的 mdb=/SAMPLE 与模型服务 mdb=/ALL 不一致"
            )
            .form(),
            FailForm::IdentityMismatch
        );
        assert_eq!(Failure::new("timeout", "").form(), FailForm::Timeout);
        assert_eq!(Failure::new("internal", "").form(), FailForm::Internal);
        // 「自动同步接管 · 422」与「已有任务在跑 · 409」随合流退役
        // （gen-model ADR-011 §12）：这两个 code 若意外出现，不再有专属画面，
        // 归 internal，原始 message 收进详情。
        assert_eq!(
            Failure::new("precondition", "sync_live=true 时不允许手动更新").form(),
            FailForm::Internal
        );
        assert_eq!(
            Failure::new("conflict", "已有任务正在执行: mu-3c81").form(),
            FailForm::Internal
        );
        // message 里带 sync_live 但 code 是 internal 的，仍然归 internal。
        assert_eq!(
            Failure::new("internal", "sync_live").form(),
            FailForm::Internal
        );
    }

    /// 「服务以 direct 形态运行、没起 worker」自己是一格（S2-D）：它不是出错、
    /// 也不是身份不对——预览照常，只是执行没人消化。归 internal 会把它讲成故障，
    /// 归 identity_mismatch 会让人去查一处并不存在的配置错位。
    #[test]
    fn worker_disabled_is_its_own_fail_form() {
        let failure = Failure::new(
            "worker_disabled",
            "模型服务以 direct 形态运行，未启动数据批次 worker",
        );
        assert_eq!(failure.form(), FailForm::WorkerDisabled);
        assert_eq!(
            FailForm::WorkerDisabled.tone(),
            Status::Warn,
            "不是故障，是形态"
        );
        assert!(FailForm::WorkerDisabled.body().contains("预览照常"));
        assert!(
            FailForm::WorkerDisabled
                .body()
                .contains("没有任何数据被改动")
        );
    }

    /// 结果摘要按契约字段名解。`status` 是 snake_case 的串，不是数字；
    /// **`batch` 是单数**——合流之后一个任务就是一个数据批次。
    /// 消费者是任务队列视图（`TaskEntry.result`），向导不再跟任何一次运行。
    #[test]
    fn outcome_decodes_the_partial_terminal_contract() {
        let outcome: Outcome = serde_json::from_str(
            r#"{
                "project":"AMS-8009","status":"partial",
                "batch":{"dbnum":8000,"db_type":"DESI","file_path":"a",
                    "start_sesno":1024,"end_sesno":1034,"status":"applied",
                    "message":null,"merged_sesnos":[1032,1033,1034],
                    "changed_elements":512},
                "units":[{"dbnum":8000,"root_refno":"24381/2","noun":"EQUI",
                    "status":"failed","attempts":3,"message":"写入生成结果失败"}],
                "warnings":["14 个变更无法解析合法生成根，跳过模型生成"]
            }"#,
        )
        .expect("契约 payload 应当能解出来");

        assert_eq!(outcome.status, RunStatus::Partial);
        let batch = outcome.batch.expect("数据批次摘要必须解得出来");
        assert_eq!(batch.status, BatchStatus::Applied);
        assert_eq!(batch.merged_sesnos, vec![1032, 1033, 1034]);
        assert_eq!(outcome.units[0].status, UnitStatus::Failed);
        assert_eq!(outcome.units[0].attempts, 3);
    }

    /// 入队回执是数组不是单个 task_id：一次扫描可能排下好几个库，也可能一行都没排。
    /// 解成旧的 `{task_id, kind, state}` 会**直接反序列化失败**，界面落到一个
    /// 看不懂的 internal 错误页——这正是合流之后向导那枚按钮坏掉的形态。
    #[test]
    fn execute_returns_an_enqueue_receipt_not_a_single_task() {
        let receipt: Enqueued = serde_json::from_str(
            r#"{"project":"AMS-8009","scanned":3,
                "enqueued":[{"task_id":"db-7997-1","dbnum":7997,"db_type":"DESI",
                    "position":1,"start_sesno":1024,"end_sesno":1038}],
                "merged":[{"task_id":"db-8000-2","dbnum":8000,"db_type":"DESI",
                    "position":2,"start_sesno":812,"end_sesno":830}],
                "already_covered":[],
                "blocked":[{"dbnum":8003,"reason":"文件回退 812 < 已应用 1005"}],
                "up_to_date":1,"warnings":[]}"#,
        )
        .expect("入队回执应当能解出来");
        let summary = receipt.summary();
        assert!(summary.contains("db7997 排第 1 位"), "{summary}");
        assert!(summary.contains("1 个并入已排队的批次"), "{summary}");
        assert!(summary.contains("db8003：文件回退"), "{summary}");

        // 一行都没排也要说得清是为什么——不然人只会以为按钮没反应。
        let idle: Enqueued =
            serde_json::from_str(r#"{"scanned":3,"up_to_date":3}"#).expect("空回执也要解得出来");
        assert!(
            idle.summary().contains("3 个已是最新"),
            "{}",
            idle.summary()
        );
    }

    /// 回执里的五个去向要与 `scanned` 算得平。
    ///
    /// 早先只在「一行都没排」的兜底分支里报 `up_to_date` 与 `already_covered`，
    /// 于是只要有任意一条入队，那两个数就整个消失：人看到「扫描 29 个库；
    /// 已入队 2 个数据批次」，剩下 27 个去哪了说不出来。
    #[test]
    fn the_enqueue_receipt_accounts_for_every_scanned_dbnum() {
        let mut receipt = Enqueued {
            scanned: 29,
            enqueued: vec![EnqueuedBatch {
                dbnum: 7997,
                position: 1,
                ..Default::default()
            }],
            already_covered: vec![8000, 8001],
            up_to_date: 25,
            blocked: vec![BlockedDbnum {
                dbnum: 8003,
                reason: "文件回退".into(),
            }],
            ..Default::default()
        };

        let summary = receipt.summary();
        assert!(summary.contains("已入队 1 个数据批次"), "{summary}");
        assert!(summary.contains("2 个已被排队中的批次覆盖"), "{summary}");
        assert!(summary.contains("25 个已是最新"), "{summary}");
        assert!(summary.contains("1 个阻断未入队"), "{summary}");
        assert!(
            !summary.contains("没交代去向"),
            "1 + 2 + 25 + 1 = 29，算得平就不该有差额：{summary}"
        );

        // 算不平的时候差额必须说出来，不许让那几个库无声消失。
        receipt.up_to_date = 20;
        let short = receipt.summary();
        assert!(short.contains("另有 5 个库回执没交代去向"), "{short}");
    }

    /// 会话区间从**水位的下一号**起算。需初始化的库契约不为它解区间，
    /// 那一行不许摆一个假的 `sesno 1 → 0`——那看起来像「有一条会话待应用」。
    #[test]
    fn the_session_window_starts_one_past_the_watermark() {
        let mut pending = db(8000, "DESI");
        pending.applied_sesno = 1023;
        pending.file_latest_sesno = 1031;
        assert_eq!(pending.window(), "sesno 1024 → 1031");

        pending.initialization_required = true;
        assert!(
            !pending.window().contains("sesno"),
            "首次导入尚无权威水位，不许摆区间：{}",
            pending.window()
        );
    }

    /// `footer_hint` 是 `match vm` 带 guard，而**臂的先后是有意义的**：确认页那一臂
    /// 比只读那一臂多一个 `state.step == Step::Confirm` 条件，两臂一调位，「任务不可
    /// 中途取消」就静默消失——而那句话是确认步存在的全部理由（ADR-0011）。编译器
    /// 不报，界面上也看不出异样。
    #[test]
    fn the_confirm_step_must_say_the_button_cannot_be_taken_back() {
        const API: &str = "http://127.0.0.1:8080";
        let vm = Vm::Ready(Preview::default());
        let mut state = State::default();

        state.step = Step::Confirm;
        let (_, hint, tone) = footer_hint(&vm, &state, API, None);
        assert_eq!(tone, Status::Warn);
        assert!(
            hint.contains("不可中途取消"),
            "确认页丢了不可撤销警告：{hint}"
        );

        state.step = Step::Preview;
        let (_, hint, tone) = footer_hint(&vm, &state, API, None);
        assert_eq!(tone, Status::Neutral);
        assert!(hint.contains("只读"), "{hint}");

        // 已是最新的那一份没有可按的按钮，两臂的 guard 都不该命中。
        let fresh = Vm::Ready(Preview {
            up_to_date: true,
            ..Default::default()
        });
        state.step = Step::Confirm;
        let (_, hint, _) = footer_hint(&fresh, &state, API, None);
        assert!(hint.contains(API), "{hint}");
    }

    /// 服务此刻不消化执行请求（direct 无 worker）时，脚注要说的是**为什么按钮灰着**，
    /// 而且要顶掉「预览为只读」与「不可中途取消」——人对着一份预览、按钮却灰着，
    /// 唯一用得上的信息就是原因。已是最新的那一份没有按钮，不该被顶掉。
    #[test]
    fn a_blocked_service_takes_over_the_footer_hint_on_both_steps() {
        const API: &str = "http://127.0.0.1:8080";
        const REASON: &str = "模型服务以 direct 形态运行，未启动数据批次 worker";
        let vm = Vm::Ready(Preview::default());
        let mut state = State::default();

        for step in [Step::Preview, Step::Confirm] {
            state.step = step;
            let (_, hint, tone) = footer_hint(&vm, &state, API, Some(REASON));
            assert_eq!(tone, Status::Warn, "{step:?}");
            assert!(hint.contains(REASON), "{step:?}: {hint}");
            assert!(!hint.contains("只读"), "{step:?}: {hint}");
        }

        let fresh = Vm::Ready(Preview {
            up_to_date: true,
            ..Default::default()
        });
        let (_, hint, _) = footer_hint(&fresh, &state, API, Some(REASON));
        assert!(hint.contains(API), "已是最新没有按钮可灰：{hint}");
    }

    /// 副标题必须点名 MDB。范围由 MDB 定，而服务端与客户端各有一份 `mdb_name`
    /// 配置——不把它说出来，两边错开时界面一声不响。
    #[test]
    fn the_header_names_the_mdb_the_scope_was_resolved_against() {
        let state = State::default();
        let scoped = Vm::Ready(Preview {
            project: "AMS-8009".into(),
            mdb: "/ALL".into(),
            ..Default::default()
        });
        let (_, sub, _, _) = header_text(&scoped, &state, "/ALL", None);
        assert!(sub.contains("AMS-8009") && sub.contains("/ALL"), "{sub}");

        let (_, sub, _, _) = header_text(&Vm::Loading, &state, "/ALL", Some(true));
        assert!(
            sub.contains("MDB /ALL") && sub.contains("自动同步已开启"),
            "{sub}"
        );

        let starting = Vm::Starting(match &scoped {
            Vm::Ready(preview) => preview.clone(),
            _ => unreachable!(),
        });
        let (_, sub, _, _) = header_text(&starting, &state, "/ALL", Some(false));
        assert!(
            sub.contains("/ALL") && sub.contains("自动同步已关闭"),
            "{sub}"
        );

        // 服务端没回显时不许编一个出来。
        let bare = Vm::Ready(Preview {
            project: "AMS-8009".into(),
            ..Default::default()
        });
        let (_, sub, _, _) = header_text(&bare, &state, "", None);
        assert!(!sub.contains("MDB"), "回显缺席时不许摆一个假 MDB：{sub}");
    }

    /// 「预览一刷新就退回第一步」：确认页上摆的是刚才那份的数字。判定靠相位切换，
    /// 而 `sync` 是每一帧都在跑的——同一相位内 sync 多少次都不该把人踢回去。
    #[test]
    fn a_fresh_preview_drops_the_user_back_to_the_first_step() {
        let mut state = State::default();
        let ready = Vm::Ready(Preview {
            project: "AMS-8009".into(),
            ..Default::default()
        });

        state.sync(&ready);
        assert_eq!(state.step, Step::Preview);

        state.step = Step::Confirm;
        state.detail_open = true;
        state.sync(&ready);
        state.sync(&ready);
        assert_eq!(
            state.step,
            Step::Confirm,
            "同一相位内反复 sync 不该把人踢回第一步"
        );

        // 重新预览再回来：这是新的一份，确认页上那些数字已经不作数了。
        state.sync(&Vm::Loading);
        state.sync(&ready);
        assert_eq!(state.step, Step::Preview);
        assert!(!state.detail_open);
    }

    /// 预览失败之后标题栏仍要报得出项目名——`Failed` 里没有 project，它靠的是
    /// 缓存的上一份预览。
    #[test]
    fn the_cached_preview_still_names_the_project_after_a_failure() {
        let mut state = State::default();
        state.sync(&Vm::Ready(Preview {
            project: "AMS-8009".into(),
            ..Default::default()
        }));

        let failed = Vm::Failed(Failure::new("timeout", "模型服务没有响应"));
        state.sync(&failed);
        assert_eq!(project_of(&failed, &state), "AMS-8009");

        // 从没成功过的时候才退到那个占位串。
        assert_eq!(project_of(&failed, &State::default()), "当前项目");
    }

    /// `toggled` 记的是「与默认相反」的那些行，判定是异或。写成 `==` 的话有一半
    /// 的行展开行为会反过来，而两种默认在同一张树上都存在（会跑的库默认摊开、
    /// 不跑的收起）——只看其中一种是发现不了的。
    #[test]
    fn toggling_records_only_the_rows_that_differ_from_their_default() {
        let mut state = State::default();
        assert!(state.open_row("db8000", true), "没人动过就是它的默认");
        assert!(!state.open_row("db8191", false));

        state.toggle_row("db8000");
        assert!(
            !state.open_row("db8000", true),
            "默认展开的行 toggle 一次要收起来"
        );
        state.toggle_row("db8000");
        assert!(state.open_row("db8000", true), "再 toggle 一次回到默认");

        state.toggle_row("db8191");
        assert!(
            state.open_row("db8191", false),
            "默认折叠的行 toggle 一次要展开"
        );
    }

    fn site(refno: &str, name: &str) -> SitePreview {
        SitePreview {
            site_refno: refno.into(),
            name: name.into(),
            ..Default::default()
        }
    }

    /// 勾选状态按 dbnum 记（Q1 同库联动：勾任何一个 SITE = 同库全亮，无半选），
    /// 折算出的 `dbnums[]` 只含勾中的**会跑**批次，且**总是显式名单**——
    /// 全不勾时是空表而不是 `None`，`None` 会被服务端当全范围执行（ADR-020）。
    #[test]
    fn selected_dbnums_lists_exactly_the_checked_runnable_batches() {
        let mut pending = db(8000, "DESI");
        pending.sessions.push(SessionPreview::default());
        pending.sites = vec![site("8000/1", "/1WCC-PIPE"), site("8000/2", "/1WCC-EQUI")];
        let mut init = db(8021, "DESI");
        init.initialization_required = true;
        let mut blocked = db(8003, "DESI");
        blocked.anomaly = Some(FileAnomaly::Missing { path: "a".into() });
        blocked.blocked = true;
        let preview = Preview {
            dbnums: vec![
                pending,
                init,
                blocked,
                unreachable_db(7015),
                db(8191, "CATA"),
                db(8044, "DESI"), // 已是最新：不跑，也就无所谓勾不勾
            ],
            ..Default::default()
        };

        let mut state = State::default();
        assert_eq!(
            state.selected_dbnums(&preview),
            vec![8000, 8021],
            "默认全勾：会跑的两批（含首次导入），阻断/够不着/范围外/已最新都不在名单里"
        );

        // 界面上点的是 SITE 行的勾选框，落到状态里是库级 toggle——同库另一个
        // SITE 没有独立状态可言，这就是联动的实现事实。
        state.toggle_db(8000);
        assert!(!state.db_checked(8000));
        assert_eq!(state.selected_dbnums(&preview), vec![8021]);

        state.toggle_db(8021);
        assert_eq!(
            state.selected_dbnums(&preview),
            Vec::<u32>::new(),
            "全不勾要交出空表：让服务端一个批次都不排、只等 worker 收重试"
        );

        state.toggle_db(8000);
        assert_eq!(state.selected_dbnums(&preview), vec![8000], "再点一次回来");
    }

    /// §2-G 工具行 / 主按钮 / 确认页共用的折算口径：SITE 数只数有名有姓的桶
    /// （归属未知桶没有勾选框），批次数含首次导入库，交付单元按 `db.units`
    /// 去重口径（挪动的单元在两个 SITE 桶里各出现一次，按桶相加会虚高）。
    #[test]
    fn selection_counts_sites_by_name_and_batches_by_database() {
        let mut pending = db(8000, "DESI");
        pending.sessions.push(SessionPreview::default());
        pending.sites = vec![
            site("8000/1", "/1WCC-PIPE"),
            site("8000/2", "/1WCC-EQUI"),
            site("", ""), // 归属未知桶
        ];
        pending.units = vec![
            UnitPreview {
                will_generate: true,
                ..Default::default()
            },
            UnitPreview::default(),
        ];
        pending.transform_targets = vec![TransformTargetPreview::default()];
        let mut init = db(8021, "DESI");
        init.initialization_required = true;
        let preview = Preview {
            dbnums: vec![pending, init],
            ..Default::default()
        };

        let mut state = State::default();
        let all = selection(&preview, &state);
        assert_eq!(
            (all.site_total, all.site_checked),
            (2, 2),
            "未知桶不算 SITE"
        );
        assert_eq!((all.batches_total, all.batches), (2, 2), "首次导入算一批");
        assert_eq!(all.units, 2, "交付单元按库去重，不按 SITE 桶相加");
        assert_eq!(all.generating, 1);
        assert_eq!(all.transform_targets, 1);

        state.toggle_db(8000);
        let partial = selection(&preview, &state);
        assert_eq!((partial.site_total, partial.site_checked), (2, 0));
        assert_eq!(
            (partial.batches_total, partial.batches),
            (2, 1),
            "取消 db8000 之后剩首次导入那一批"
        );
        assert_eq!(
            (partial.units, partial.generating, partial.transform_targets),
            (0, 0, 0),
            "未勾选库的单元与位姿目标不许再进确认页的数字"
        );
    }

    /// 预览一刷新，勾选集要跟着归零（回到默认全勾）——旧勾选对着的是上一份
    /// 预览里的库，新一份里同一个 dbnum 的内容可能已经完全不同。
    #[test]
    fn a_fresh_preview_resets_the_checkboxes_to_all_on() {
        let mut state = State::default();
        let ready = Vm::Ready(Preview {
            project: "AMS-8009".into(),
            ..Default::default()
        });

        state.sync(&ready);
        state.toggle_db(8000);
        state.sync(&ready);
        assert!(
            !state.db_checked(8000),
            "同一相位内反复 sync 不许把人勾好的选择洗掉"
        );

        state.sync(&Vm::Loading);
        state.sync(&ready);
        assert!(state.db_checked(8000), "重新预览回来，勾选回到默认全勾");
    }
}
