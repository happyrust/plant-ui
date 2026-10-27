//! 「增量记录」页签：`/tasks` 里每一条批次的终态账本。
//!
//! 与「任务队列」分工：队列页按 dbnum 泳道看「此刻在跑什么、积了多少、还欠着
//! 什么」；本页签按时间轴看「每一轮增量到底改了什么」——保存窗口、增删改三数、
//! 逐单元发布方式（整根重算 / 刚体前移）、阶段耗时、房间轮的面板 / 成员计数。
//!
//! 数据是队列轮询那份快照（`QueuePoll` 一直在打，状态栏计数靠它），`vm.tasks` 逐条画出来就是账本；
//! 更早的只在人按「加载更早」时按需翻（`/tasks?before=`，进 `vm.archived`），元素级清单在展开明细里按需取
//! （`/tasks/{id}/changes`，进 `vm.changes`）——服务端没给的就说不到，不猜。
//!
//! 2026-10-23 重设计（`design/增量记录.pen`）：按日期分组；库 / 状态 / 关键字筛和
//! 「只看有变更」；增删改三数上色、单元进度条；失败批次和补排它的那条串成链；展开明细里
//! 画 `steps[]` 的阶段耗时比例条、告警折叠、单元行带定位 / 显示 / 房间三个动作。服务端 2026-10-24 起给的
//! 单元级房间前后、几何前后有字段才画；元素级变化清单（`changes_total` + `GET /tasks/{id}/changes`）展开明细里按需取，
//! 服务端没给条数就不画入口、不猜。
//!
//! 行的可点展开态在 `State.open` 里，与队列页签的展开集**不共用**：同一条任务
//! 在两处各开各的，互不串台。

use std::collections::{BTreeSet, HashMap, HashSet};
use std::str::FromStr;

use chrono::{DateTime, Datelike, Local, NaiveDate};
use egui::{Align, Color32, CornerRadius, Margin, Rect, RichText, ScrollArea, Sense, Ui, vec2};
use egui_phosphor::regular as ph;

use crate::model_update::{BatchStatus, UnitKind, UnitStatus};
use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Status, Tokens, radius, space};
use crate::style::widgets::{self, PaneNote, PaneState};
use crate::task_queue::{
    ChangesState, ElementChange, KIND_DATA_BATCH, KIND_MODEL_DRAIN, KIND_MODEL_REBUILD,
    KIND_ROOM_REBUILD, KIND_ROOM_RECALC, TaskEntry, TaskStep, Vm,
};
use crate::{Cmd, ModelAction, RefU64};

/// 记录页签自己的绘制状态（展开集 + 筛选）。属于界面本身而非数据，不进 Vm。
#[derive(Debug, Default)]
pub struct State {
    open: HashSet<String>,
    /// 告警段展开了的任务。告警默认折起——一批次八条原文会把明细挤成日志。
    warnings_open: HashSet<String>,
    /// 折起的日期分组。
    collapsed_days: HashSet<NaiveDate>,
    filter: Filter,
    state_filter: StateFilter,
    /// `None` = 全部库。
    dbnum: Option<u32>,
    query: String,
    only_changed: bool,
}

/// 类型筛：队列页里的任务不止数据批次一种，房间轮与整库重建同样是「一轮增量」
/// 的账。筛选项按服务端 `kind` 串分桶，认不出的 kind 归「全部」——藏掉一条
/// 看不懂的记录比多显示一行更糟。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Filter {
    #[default]
    All,
    DataBatch,
    Room,
    ModelRebuild,
}

impl Filter {
    fn label(self) -> &'static str {
        match self {
            Filter::All => "全部",
            Filter::DataBatch => "数据批次",
            Filter::Room => "房间",
            Filter::ModelRebuild => "模型重建",
        }
    }

    fn matches(self, kind: &str) -> bool {
        match self {
            Filter::All => true,
            Filter::DataBatch => kind == KIND_DATA_BATCH,
            Filter::Room => kind == KIND_ROOM_RECALC || kind == KIND_ROOM_REBUILD,
            Filter::ModelRebuild => kind == KIND_MODEL_REBUILD,
        }
    }
}

/// 状态筛。「失败」连部分成功一起收：部分成功意味着有单元没生成出来，排查时要一起看。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum StateFilter {
    #[default]
    All,
    Succeeded,
    Failed,
    Active,
}

impl StateFilter {
    const ALL: [StateFilter; 4] = [
        StateFilter::All,
        StateFilter::Succeeded,
        StateFilter::Failed,
        StateFilter::Active,
    ];

    fn label(self) -> &'static str {
        match self {
            StateFilter::All => "状态：全部",
            StateFilter::Succeeded => "状态：成功",
            StateFilter::Failed => "状态：失败 / 部分",
            StateFilter::Active => "状态：进行中",
        }
    }

    fn matches(self, state: &str) -> bool {
        match self {
            StateFilter::All => true,
            StateFilter::Succeeded => state == "succeeded",
            StateFilter::Failed => matches!(state, "failed" | "partial"),
            StateFilter::Active => matches!(state, "queued" | "running"),
        }
    }
}

/// 失败批次与补排它的那条之间的链。服务端（2026-10-24 起）在排出后继时自己写 `superseded_by` /
/// `supersedes`，有字段就读字段；老服务端没有，那时才按窗口推，推出来的记在 `inferred` 里、界面上标一个「推」。
#[derive(Debug, Default)]
struct Chains {
    /// 失败批次 → 覆盖了它的补排批次。
    superseded_by: HashMap<String, String>,
    /// 补排批次 → 它覆盖的那些失败批次。
    supersedes: HashMap<String, Vec<String>>,
    /// 哪些失败批次的链是客户端推的（服务端没给字段）。
    inferred: HashSet<String>,
}

pub fn show(ui: &mut Ui, t: &Tokens, d: Density, vm: &Vm, state: &mut State, cmds: &mut Vec<Cmd>) {
    egui::Frame::new().fill(t.bg_panel).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        ui.spacing_mut().item_spacing.y = 0.0;

        if !vm.loaded {
            widgets::pane_note(
                ui,
                t,
                d,
                PaneNote {
                    state: PaneState::Uninit,
                    icon: ph::GIT_COMMIT,
                    text: "还没连上模型服务：增量账本跟队列轮询同一路，连上后这里就有内容",
                    detail: None,
                    retry: false,
                },
            );
            return;
        }
        if let Some(error) = vm.tasks_error.as_deref() {
            degraded_banner(
                ui,
                t,
                d,
                &format!("任务表暂时读不到，下面是上一次取到的快照：{error}"),
            );
        }

        // 与队列页同一口径（task_queue::mine）：项目字段缺席按显示处理——
        // 一个 gen-model 进程只服务一个项目，藏掉一条说不清出身的行更糟。
        // 「加载更早」翻回来的终态行接在轮询页后面；同 id 的以轮询页为准（那份更新）。
        let mine: Vec<&TaskEntry> = vm
            .tasks
            .iter()
            .chain(
                vm.archived
                    .iter()
                    .filter(|older| !vm.tasks.iter().any(|t| t.task_id == older.task_id)),
            )
            .filter(|entry| {
                entry.project.is_empty() || vm.project.is_empty() || entry.project == vm.project
            })
            .collect();
        let dbnums: BTreeSet<u32> = mine.iter().filter_map(|entry| entry.dbnum).collect();
        let chains = chains(&mine);
        let query = state.query.trim().to_lowercase();
        let entries: Vec<&TaskEntry> = mine
            .iter()
            .copied()
            .filter(|entry| state.filter.matches(&entry.kind))
            .filter(|entry| state.state_filter.matches(&entry.state))
            .filter(|entry| state.dbnum.is_none_or(|dbnum| entry.dbnum == Some(dbnum)))
            .filter(|entry| !state.only_changed || has_changes(entry))
            .filter(|entry| query.is_empty() || matches_query(entry, &query))
            .collect();

        toolbar(ui, t, d, entries.len(), mine.len(), &dbnums, state);
        if mine.is_empty() {
            widgets::pane_note(
                ui,
                t,
                d,
                PaneNote {
                    state: PaneState::Empty,
                    icon: ph::GIT_COMMIT,
                    text: "还没有增量批次落地过：保存窗口进来之后，每一轮的账在这里按时间排",
                    detail: None,
                    retry: false,
                },
            );
            return;
        }
        if entries.is_empty() {
            widgets::pane_note(
                ui,
                t,
                d,
                PaneNote {
                    state: PaneState::Empty,
                    icon: ph::FUNNEL,
                    text: "这组筛选下没有记录",
                    detail: Some("换一个库 / 状态，或清掉搜索词和「只看有变更」"),
                    retry: false,
                },
            );
            return;
        }

        ScrollArea::vertical()
            .id_salt("increment-records")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let mut current_day: Option<NaiveDate> = None;
                let mut hidden = false;
                for entry in &entries {
                    let day = anchor(entry).map(|at| at.date_naive());
                    if day != current_day {
                        current_day = day;
                        let collapsed = day.is_some_and(|day| state.collapsed_days.contains(&day));
                        let same_day: Vec<&TaskEntry> = entries
                            .iter()
                            .copied()
                            .filter(|other| anchor(other).map(|at| at.date_naive()) == day)
                            .collect();
                        if day_header(ui, t, d, day, &same_day, collapsed)
                            && let Some(day) = day
                            && !state.collapsed_days.remove(&day)
                        {
                            state.collapsed_days.insert(day);
                        }
                        hidden = collapsed;
                    }
                    if hidden {
                        continue;
                    }
                    record_row(ui, t, d, entry, &chains, vm, state, cmds);
                }
                footer(ui, t, d, vm, cmds);
            });
    });
}

// ---------------------------------------------------------------- 筛选

fn has_changes(entry: &TaskEntry) -> bool {
    let outcome = entry.result.as_ref();
    outcome
        .and_then(|o| o.batch.as_ref())
        .is_some_and(|b| b.changed_elements > 0)
        || outcome.is_some_and(|o| !o.units.is_empty())
        || entry.detail.is_some_and(|c| {
            c.panels + c.elements + c.dead_letters > 0 && entry.kind == KIND_ROOM_RECALC
        })
}

/// 关键字筛认任务号、库号、会话号、单元的 refno / noun、批次消息。全部小写比。
fn matches_query(entry: &TaskEntry, query: &str) -> bool {
    if entry.task_id.to_lowercase().contains(query) {
        return true;
    }
    if entry
        .dbnum
        .is_some_and(|dbnum| dbnum.to_string().contains(query))
    {
        return true;
    }
    if entry.end_sesno.is_some_and(|s| s.to_string() == query)
        || entry.start_sesno.is_some_and(|s| s.to_string() == query)
    {
        return true;
    }
    let Some(outcome) = entry.result.as_ref() else {
        return false;
    };
    outcome.units.iter().any(|unit| {
        unit.root_refno.to_lowercase().contains(query)
            || unit.root_refno.replace('/', "_").contains(query)
            || unit.noun.to_lowercase().contains(query)
    }) || outcome
        .batch
        .as_ref()
        .and_then(|b| b.message.as_deref())
        .is_some_and(|m| m.to_lowercase().contains(query))
}

/// 失败批次与补排批次的链：同库的数据批次里，一条成功（或部分成功）的窗口盖住了
/// 失败批次的右端、并且比它后入队，就是补排它的那条；多条满足时取最早的。
/// 典型场景：E3D 还在写时 watcher 先看到了半个会话，提交前身份复核拦下，5 s 后重扫补排。
///
/// 先读服务端字段（`superseded_by` / `supersedes`，gen-model 2026-10-24）；只有两边都没给字段的失败批次
/// 才按窗口推，且只推 `failed`（与服务端同一口径：`partial` 是数据已应用、下一窗不重覆盖）。
fn chains(entries: &[&TaskEntry]) -> Chains {
    let mut chains = Chains::default();
    for entry in entries {
        if let Some(successor) = entry.superseded_by.as_deref() {
            chains
                .superseded_by
                .insert(entry.task_id.clone(), successor.to_owned());
            // 两边都带字段时同一条链会被看到两次（后继先到还是失败行先到都有），只记一次。
            let list = chains.supersedes.entry(successor.to_owned()).or_default();
            if !list.contains(&entry.task_id) {
                list.push(entry.task_id.clone());
            }
        }
        for covered in &entry.supersedes {
            chains
                .superseded_by
                .entry(covered.clone())
                .or_insert_with(|| entry.task_id.clone());
            let list = chains.supersedes.entry(entry.task_id.clone()).or_default();
            if !list.contains(covered) {
                list.push(covered.clone());
            }
        }
    }
    // 服务端给过任何一条链就是新服务端：没连上的失败批次就是没被补排，不再猜。
    if !chains.superseded_by.is_empty() {
        return chains;
    }
    for failed in entries
        .iter()
        .filter(|e| e.kind == KIND_DATA_BATCH && e.state == "failed")
    {
        let (Some(dbnum), Some(end)) = (failed.dbnum, failed.end_sesno) else {
            continue;
        };
        let successor = entries
            .iter()
            .filter(|e| {
                e.task_id != failed.task_id
                    && e.kind == KIND_DATA_BATCH
                    && e.dbnum == Some(dbnum)
                    && matches!(e.state.as_str(), "succeeded" | "partial")
                    && e.start_sesno.is_some_and(|s| s <= end)
                    && e.end_sesno.is_some_and(|s| s >= end)
                    && created_order(e, failed) == std::cmp::Ordering::Greater
            })
            .min_by(|a, b| created_order(a, b));
        if let Some(successor) = successor {
            chains
                .superseded_by
                .insert(failed.task_id.clone(), successor.task_id.clone());
            chains
                .supersedes
                .entry(successor.task_id.clone())
                .or_default()
                .push(failed.task_id.clone());
            chains.inferred.insert(failed.task_id.clone());
        }
    }
    chains
}

/// 两条任务按入队时刻排先后：两边都解得出就比时刻，解不出的按 RFC3339 原串比
/// （同一服务端发的同一时区，字典序即时序）。
fn created_order(a: &TaskEntry, b: &TaskEntry) -> std::cmp::Ordering {
    match (parse(&a.created_at), parse(&b.created_at)) {
        (Some(a), Some(b)) => a.cmp(&b),
        _ => a.created_at.cmp(&b.created_at),
    }
}

// ---------------------------------------------------------------- 工具栏与分组头

fn toolbar(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    shown: usize,
    total: usize,
    dbnums: &BTreeSet<u32>,
    state: &mut State,
) {
    let (_, rect) = ui.allocate_space(vec2(ui.available_width(), d.px(40.0)));
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter().rect_filled(rect, 0.0, t.bg_header);
    ui.scope_builder(
        egui::UiBuilder::new()
            .layout(egui::Layout::left_to_right(Align::Center))
            .max_rect(rect.shrink2(vec2(d.px(space::S3), d.px(4.0))))
            .id(egui::Id::new("incr-toolbar")),
        |ui| {
            ui.spacing_mut().item_spacing.x = d.px(space::S1);
            for filter in [
                Filter::All,
                Filter::DataBatch,
                Filter::Room,
                Filter::ModelRebuild,
            ] {
                if ui
                    .add(widgets::chip(t, d, filter.label(), state.filter == filter))
                    .clicked()
                {
                    state.filter = filter;
                }
            }
            ui.add_space(d.px(space::S2));
            let dbnum_label = match state.dbnum {
                Some(dbnum) => format!("库：{dbnum}"),
                None => "库：全部".to_string(),
            };
            egui::ComboBox::from_id_salt("incr-dbnum")
                .selected_text(RichText::new(dbnum_label).font(Font::meta(d)))
                .width(d.px(96.0))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut state.dbnum, None, "全部库");
                    for dbnum in dbnums {
                        ui.selectable_value(&mut state.dbnum, Some(*dbnum), format!("库 {dbnum}"));
                    }
                });
            egui::ComboBox::from_id_salt("incr-state")
                .selected_text(RichText::new(state.state_filter.label()).font(Font::meta(d)))
                .width(d.px(128.0))
                .show_ui(ui, |ui| {
                    for candidate in StateFilter::ALL {
                        ui.selectable_value(&mut state.state_filter, candidate, candidate.label());
                    }
                });
            ui.add(
                egui::TextEdit::singleline(&mut state.query)
                    .hint_text("搜 refno / 任务号 / 根名")
                    .desired_width(d.px(190.0))
                    .font(Font::meta(d)),
            );
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = d.px(space::S2);
                let count = if shown == total {
                    format!("{total} 条记录")
                } else {
                    format!("{shown} / {total} 条记录")
                };
                ui.label(RichText::new(count).font(Font::meta(d)).color(t.text_muted));
                ui.label(
                    RichText::new("只看有变更")
                        .font(Font::meta(d))
                        .color(t.text_secondary),
                );
                ui.add(widgets::toggle(t, d, &mut state.only_changed));
            });
        },
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(1.0, t.border),
    );
}

/// 日期分组头：日期 + 这一天的汇总（批次数 / 变更件数 / 失败数 / 耗时合计）。点击折起。
/// 解不出时刻的批次归「时间未知」一组，不藏。返回是否被点击。
fn day_header(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    day: Option<NaiveDate>,
    entries: &[&TaskEntry],
    collapsed: bool,
) -> bool {
    let (_, rect) = ui.allocate_space(vec2(ui.available_width(), d.px(28.0)));
    let resp = ui.interact(rect, egui::Id::new(("incr-day", day)), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp.clicked();
    }
    ui.painter().rect_filled(
        rect,
        0.0,
        if resp.hovered() {
            t.bg_hover
        } else {
            t.bg_header
        },
    );
    let failed = entries
        .iter()
        .filter(|e| matches!(e.state.as_str(), "failed" | "partial"))
        .count();
    let changed: u64 = entries
        .iter()
        .filter_map(|e| e.result.as_ref().and_then(|o| o.batch.as_ref()))
        .map(|b| b.changed_elements)
        .sum();
    let elapsed: std::time::Duration = entries.iter().filter_map(|e| elapsed_of(e)).sum();
    let mut summary = vec![format!("{} 批次", entries.len())];
    if changed > 0 {
        summary.push(format!("变更 {changed} 件"));
    }
    if failed > 0 {
        summary.push(format!("{failed} 失败"));
    }
    if !elapsed.is_zero() {
        summary.push(format!("耗时合计 {}", clock(elapsed)));
    }
    ui.scope_builder(
        egui::UiBuilder::new()
            .layout(egui::Layout::left_to_right(Align::Center))
            .max_rect(rect.shrink2(vec2(d.px(space::S4), 0.0)))
            .id(egui::Id::new(("incr-day-ui", day))),
        |ui| {
            ui.spacing_mut().item_spacing.x = d.px(space::S2);
            ui.label(
                RichText::new(if collapsed {
                    ph::CARET_RIGHT
                } else {
                    ph::CARET_DOWN
                })
                .font(Font::meta(d))
                .color(t.text_muted),
            );
            ui.label(
                RichText::new(ph::CALENDAR_BLANK)
                    .font(Font::meta(d))
                    .color(t.text_secondary),
            );
            ui.label(
                RichText::new(day_label(day))
                    .font(Font::meta_strong(d))
                    .color(t.text_secondary),
            );
            ui.label(
                RichText::new(summary.join(" · "))
                    .font(Font::meta(d))
                    .color(t.text_muted),
            );
        },
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(1.0, t.border),
    );
    resp.clicked()
}

fn day_label(day: Option<NaiveDate>) -> String {
    let Some(day) = day else {
        return "时间未知".to_string();
    };
    let today = Local::now().date_naive();
    let md = day.format("%m-%d");
    if day == today {
        format!("今天 · {md}")
    } else if today.pred_opt() == Some(day) {
        format!("昨天 · {md}")
    } else {
        let weekday = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"]
            [day.weekday().num_days_from_monday() as usize];
        if day.year() == today.year() {
            format!("{md} · {weekday}")
        } else {
            format!("{} · {weekday}", day.format("%Y-%m-%d"))
        }
    }
}

/// 底部：服务端说还有更早的就给「加载更早」；翻到底了说到底；老服务端不落盘也不给 `has_more`，就说它给到哪。
fn footer(ui: &mut Ui, t: &Tokens, d: Density, vm: &Vm, cmds: &mut Vec<Cmd>) {
    let (_, rect) = ui.allocate_space(vec2(ui.available_width(), d.px(32.0)));
    if !ui.is_rect_visible(rect) {
        return;
    }
    let more = match vm.archive_has_more {
        Some(more) => more,
        None => vm.tasks_has_more,
    };
    ui.scope_builder(
        egui::UiBuilder::new()
            .layout(egui::Layout::left_to_right(Align::Center))
            .max_rect(rect.shrink2(vec2(d.px(space::S4), 0.0)))
            .id(egui::Id::new("incr-footer")),
        |ui| {
            ui.spacing_mut().item_spacing.x = d.px(space::S2);
            if more {
                let label = if vm.archive_loading {
                    "加载中…"
                } else {
                    "加载更早的记录"
                };
                if ui
                    .add_enabled(
                        !vm.archive_loading,
                        widgets::button(t, d, label).icon(ph::CLOCK_COUNTER_CLOCKWISE),
                    )
                    .clicked()
                {
                    cmds.push(Cmd::LoadOlderTasks);
                }
                ui.label(
                    RichText::new("服务端还有更早的批次（含重启前落盘的）")
                        .font(Font::meta(d))
                        .color(t.text_muted),
                );
            } else if !vm.archived.is_empty() {
                ui.label(
                    RichText::new(format!(
                        "已到最早一条；共翻回 {} 条更早的记录。",
                        vm.archived.len()
                    ))
                    .font(Font::meta(d))
                    .color(t.text_muted),
                );
            } else {
                ui.label(
                    RichText::new("已是服务端给到的全部记录；更早的看不到就不猜。")
                        .font(Font::meta(d))
                        .color(t.text_muted),
                );
            }
        },
    );
}

// ---------------------------------------------------------------- 记录行

/// 一条增量记录：主行（图标 + 类型 + 库 + 保存时刻 + 会话区间 + 右侧耗时与状态芯片）、
/// 次行（增删改三数丸标 + 单元进度条 + 整根 / 刚体计数 + 房间轮计数 + 补排标记）。
/// 点行展开明细；失败批次被补排过的，行下挂一条「补排链」。
fn record_row(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    entry: &TaskEntry,
    chains: &Chains,
    vm: &Vm,
    state: &mut State,
    cmds: &mut Vec<Cmd>,
) {
    let open = state.open.contains(&entry.task_id);
    let h = d.px(56.0);
    let (_, rect) = ui.allocate_space(vec2(ui.available_width(), h));
    let row_resp = ui.interact(
        rect,
        egui::Id::new(("incr-row", &entry.task_id)),
        Sense::click(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    let bg = if open {
        t.bg_elevated
    } else if row_resp.hovered() {
        t.bg_hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, radius::SM, bg);
    if row_resp.clicked() {
        if !state.open.remove(&entry.task_id) {
            state.open.insert(entry.task_id.clone());
        }
    }

    let pad = d.px(space::S4);
    let inner = rect.shrink2(vec2(pad, d.px(6.0)));
    let (icon, icon_color, kind_label) = kind_glyph(t, entry);
    // 主行：类型图标 + 类型名 + 库号 + 保存时刻 + 会话区间 + 右侧耗时 / 状态芯片。
    let top = Rect::from_min_size(inner.min, vec2(inner.width(), d.px(20.0)));
    ui.scope_builder(
        egui::UiBuilder::new()
            .layout(egui::Layout::left_to_right(Align::Center))
            .max_rect(top)
            .id(egui::Id::new(("incr-row-top", &entry.task_id))),
        |ui| {
            ui.spacing_mut().item_spacing.x = d.px(space::S2);
            ui.label(RichText::new(icon).font(Font::body(d)).color(icon_color));
            ui.label(
                RichText::new(kind_label)
                    .font(Font::strong(d))
                    .color(t.text_primary),
            );
            if let Some(dbnum) = entry.dbnum {
                ui.add(widgets::status_tag(
                    t,
                    d,
                    &format!("库 {dbnum}"),
                    Status::Neutral,
                ));
            }
            if let Some(saved) = saved_at(entry) {
                ui.label(
                    RichText::new(saved)
                        .font(Font::mono_micro(d))
                        .color(t.text_muted),
                );
            }
            if let (Some(start), Some(end)) = (entry.start_sesno, entry.end_sesno) {
                ui.label(
                    RichText::new(format!("会话 {start} → {end}"))
                        .font(Font::mono_micro(d))
                        .color(t.text_muted),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(if open { ph::CARET_UP } else { ph::CARET_DOWN })
                        .font(Font::meta(d))
                        .color(t.text_muted),
                );
                let (text, status) = state_chip(entry);
                ui.add(widgets::status_tag(t, d, text, status));
                if let Some(elapsed) = elapsed_of(entry) {
                    ui.label(
                        RichText::new(clock(elapsed))
                            .font(Font::mono_micro(d))
                            .color(t.text_muted),
                    );
                }
            });
        },
    );
    // 次行：增删改 + 单元进度 + 发布方式计数 + 房间轮计数 + 补排标记。
    let bottom = Rect::from_min_size(
        inner.min + vec2(0.0, d.px(24.0)),
        vec2(inner.width(), d.px(20.0)),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .layout(egui::Layout::left_to_right(Align::Center))
            .max_rect(bottom)
            .id(egui::Id::new(("incr-row-bottom", &entry.task_id))),
        |ui| {
            ui.spacing_mut().item_spacing.x = d.px(space::S2);
            ui.add_space(d.px(22.0));
            change_tags(ui, t, d, entry);
            if let Some(total) = entry.total_units {
                let done = entry.units_done.unwrap_or_default();
                ui.label(
                    RichText::new(format!("单元 {done}/{total}"))
                        .font(Font::meta(d))
                        .color(t.text_muted),
                );
                progress_bar(ui, t, d, done, total, entry);
            }
            if let Some(outcome) = entry.result.as_ref()
                && !outcome.units.is_empty()
            {
                let regen = outcome
                    .units
                    .iter()
                    .filter(|u| u.kind == UnitKind::Regen)
                    .count();
                let transform = outcome.units.len() - regen;
                let failed = outcome
                    .units
                    .iter()
                    .filter(|u| u.status == UnitStatus::Failed)
                    .count();
                ui.label(
                    RichText::new(format!("整根 {regen} · 刚体 {transform}"))
                        .font(Font::meta(d))
                        .color(t.text_muted),
                );
                if failed > 0 {
                    ui.add(widgets::status_tag(
                        t,
                        d,
                        &format!("{failed} 根失败"),
                        Status::Error,
                    ));
                }
            }
            if entry.kind == KIND_ROOM_RECALC
                && let Some(counts) = entry.detail
            {
                ui.label(
                    RichText::new(format!(
                        "面板 {} · 成员 {} · 死信 {}",
                        counts.panels, counts.elements, counts.dead_letters
                    ))
                    .font(Font::meta(d))
                    .color(t.text_muted),
                );
            }
            if entry.kind == KIND_ROOM_REBUILD
                && let Some(outcome) = entry.result.as_ref()
            {
                if let Some(trigger) = outcome.trigger.as_deref() {
                    ui.add(widgets::status_tag(
                        t,
                        d,
                        trigger_label(trigger),
                        Status::Neutral,
                    ));
                }
                if let Some(stamped) = outcome.stamped {
                    let (text, status) = if stamped {
                        ("对齐代号已盖", Status::Success)
                    } else {
                        ("对齐代号未盖", Status::Warn)
                    };
                    ui.add(widgets::status_tag(t, d, text, status));
                }
            }
            if chains.superseded_by.contains_key(&entry.task_id) {
                let label = if chains.inferred.contains(&entry.task_id) {
                    "已被补排 · 推"
                } else {
                    "已被补排"
                };
                ui.add(widgets::status_tag(t, d, label, Status::Warn));
            }
            if let Some(covered) = chains.supersedes.get(&entry.task_id) {
                ui.add(widgets::status_tag(
                    t,
                    d,
                    &format!("补排 {} 条", covered.len()),
                    Status::Info,
                ));
            }
        },
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(1.0, t.border),
    );
    if let Some(successor) = chains.superseded_by.get(&entry.task_id) {
        chain_line(ui, t, d, entry, successor, state);
    }
    if open {
        record_detail(ui, t, d, entry, chains, vm, state, cmds);
    }
}

/// 增 / 改 / 删三个丸标；非零的上色，零的按中性画。三数缺席时退回「共 N 项变化」或不画
/// （`change_breakdown` 的 None 判据与队列页同一套：缺字段与真零不可分，都不能画成「无变化」）。
fn change_tags(ui: &mut Ui, t: &Tokens, d: Density, entry: &TaskEntry) {
    let Some(batch) = entry.result.as_ref().and_then(|o| o.batch.as_ref()) else {
        return;
    };
    match batch.change_breakdown() {
        Some(b) => {
            let tag = |n: u64, s: Status| if n > 0 { s } else { Status::Neutral };
            ui.add(widgets::status_tag(
                t,
                d,
                &format!("增 {}", b.added),
                tag(b.added, Status::Success),
            ));
            ui.add(widgets::status_tag(
                t,
                d,
                &format!("改 {}", b.modified),
                tag(b.modified, Status::Info),
            ));
            ui.add(widgets::status_tag(
                t,
                d,
                &format!("删 {}", b.deleted),
                tag(b.deleted, Status::Error),
            ));
        }
        None if batch.changed_elements > 0 => {
            ui.add(widgets::status_tag(
                t,
                d,
                &format!("共 {} 项变化", batch.changed_elements),
                Status::Info,
            ));
        }
        None => {}
    }
}

/// 单元进度条：完成比例按绿画；有失败单元时按红画，让「1/1」不至于看起来都是好的。
fn progress_bar(ui: &mut Ui, t: &Tokens, d: Density, done: u32, total: u32, entry: &TaskEntry) {
    let w = d.px(72.0);
    let h = d.px(5.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let rect = Rect::from_center_size(rect.center(), vec2(w, h));
    ui.painter().rect_filled(rect, radius::SM, t.border);
    if total == 0 {
        return;
    }
    let failed = entry
        .result
        .as_ref()
        .is_some_and(|o| o.units.iter().any(|u| u.status == UnitStatus::Failed));
    let color = if failed || entry.state == "failed" {
        t.danger
    } else if entry.state == "partial" {
        t.warn
    } else {
        t.success
    };
    let ratio = (done as f32 / total as f32).clamp(0.0, 1.0);
    let filled = Rect::from_min_size(rect.min, vec2(w * ratio, h));
    ui.painter().rect_filled(filled, radius::SM, color);
}

/// 失败批次下面挂的补排链：谁覆盖了它、为什么失败，一键展开补排那条。
fn chain_line(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    failed: &TaskEntry,
    successor: &str,
    state: &mut State,
) {
    let (_, rect) = ui.allocate_space(vec2(ui.available_width(), d.px(26.0)));
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter().rect_filled(rect, 0.0, t.warn_bg);
    let reason = failed
        .result
        .as_ref()
        .and_then(|o| o.batch.as_ref())
        .and_then(|b| b.message.as_deref())
        .unwrap_or("未收口");
    ui.scope_builder(
        egui::UiBuilder::new()
            .layout(egui::Layout::left_to_right(Align::Center))
            .max_rect(rect.shrink2(vec2(d.px(space::S4), 0.0)))
            .id(egui::Id::new(("incr-chain", &failed.task_id))),
        |ui| {
            ui.spacing_mut().item_spacing.x = d.px(space::S2);
            ui.add_space(d.px(22.0));
            ui.label(
                RichText::new(ph::ARROW_ELBOW_DOWN_RIGHT)
                    .font(Font::meta(d))
                    .color(t.text_muted),
            );
            ui.label(
                RichText::new(ph::GIT_MERGE)
                    .font(Font::meta(d))
                    .color(t.warn),
            );
            // 右侧先摆链接，剩下的宽度给原因句；原因句长了截成省略号，展开明细里有全文。
            let link_w = d.px(88.0);
            let text_w = (ui.available_width() - link_w).max(d.px(80.0));
            ui.scope(|ui| {
                ui.set_max_width(text_w);
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("被后来的批次补排覆盖；这条没收口：{reason}"))
                            .font(Font::meta(d))
                            .color(t.text_secondary),
                    )
                    .truncate(),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(
                        egui::Label::new(
                            RichText::new("看补排那条")
                                .font(Font::meta(d))
                                .color(t.accent),
                        )
                        .sense(Sense::click()),
                    )
                    .clicked()
                {
                    state.open.insert(successor.to_string());
                    state.state_filter = StateFilter::All;
                }
            });
        },
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(1.0, t.border),
    );
}

// ---------------------------------------------------------------- 展开明细

/// 展开的账本明细：批次判定（应用 / 失败 / 跳过 / 被吸收）+ 并入的保存 + 增删改 +
/// 阶段耗时比例条 + 逐单元发布明细（带定位 / 显示 / 房间）+ 房间轮计数 + 告警（折叠）+
/// 时间与任务号。全是服务端回执原文的呈现，不做任何二次推断。
fn record_detail(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    entry: &TaskEntry,
    chains: &Chains,
    vm: &Vm,
    state: &mut State,
    cmds: &mut Vec<Cmd>,
) {
    egui::Frame::new()
        .fill(t.bg_header)
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(Margin {
            left: d.px(38.0) as i8,
            right: d.px(16.0) as i8,
            top: d.px(8.0) as i8,
            bottom: d.px(10.0) as i8,
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = d.px(3.0);

            let outcome = entry.result.as_ref();
            section_title(ui, t, d, "批次判定");
            if outcome.is_some_and(|o| o.is_absorbed()) {
                meta_line(ui, t, d, ph::GIT_MERGE, t.accent, "已被运行中批次吸收");
            }
            if let Some(batch) = outcome.and_then(|o| o.batch.as_ref()) {
                let (icon, color, verdict) = match batch.status {
                    BatchStatus::Applied => (ph::CHECK_CIRCLE, t.success, "已应用"),
                    BatchStatus::Failed => (ph::X_CIRCLE, t.danger, "批次失败"),
                    BatchStatus::Skipped => (ph::PROHIBIT, t.text_muted, "已跳过"),
                };
                meta_line(ui, t, d, icon, color, verdict);
                if let Some(message) = batch.message.as_deref() {
                    sub_line(ui, t, d, message, batch.status == BatchStatus::Failed);
                }
                if !batch.merged_sesnos.is_empty() {
                    meta_line(
                        ui,
                        t,
                        d,
                        ph::GIT_MERGE,
                        t.accent,
                        &format!("预览后并入 {} 次保存", batch.merged_sesnos.len()),
                    );
                    if let Some(listed) = batch.merged_save_times().as_deref().and_then(save_list) {
                        sub_line(ui, t, d, &listed, false);
                    }
                }
                // 三数缺席只报总数，绝不把缺省的 0/0/0 画成「无变化」（change_summary 的口径）。
                let summary = change_summary(entry);
                if !summary.is_empty() {
                    meta_line(ui, t, d, ph::FILE_PLUS, t.text_secondary, &summary);
                }
            } else if entry.kind == KIND_DATA_BATCH && !entry.terminal() {
                meta_line(ui, t, d, ph::CLOCK, t.text_muted, "还没收口，终态摘要未到");
            } else if entry.kind == KIND_ROOM_REBUILD {
                // 整库房间重算没有 batch：它的判定是「对齐代号盖上没有」+ 服务端那一句原文。
                match outcome.and_then(|o| o.stamped) {
                    Some(true) => meta_line(
                        ui,
                        t,
                        d,
                        ph::CHECK_CIRCLE,
                        t.success,
                        "整库房间边已重算，对齐代号已盖",
                    ),
                    Some(false) => meta_line(
                        ui,
                        t,
                        d,
                        ph::X_CIRCLE,
                        t.danger,
                        "整库房间重算没盖上对齐代号，欠账留给下个任务",
                    ),
                    None if entry.terminal() => {
                        meta_line(ui, t, d, ph::QUESTION, t.text_muted, "服务端没说盖上没有")
                    }
                    None => meta_line(ui, t, d, ph::CLOCK, t.text_muted, "正在整库重算房间边"),
                }
                if let Some(trigger) = outcome.and_then(|o| o.trigger.as_deref()) {
                    sub_line(
                        ui,
                        t,
                        d,
                        &format!("触发：{}", trigger_label(trigger)),
                        false,
                    );
                }
                if let Some(detail) = outcome.and_then(|o| o.detail.as_deref()) {
                    sub_line(
                        ui,
                        t,
                        d,
                        detail,
                        outcome.and_then(|o| o.stamped) == Some(false),
                    );
                }
            }
            if let Some(covered) = chains.supersedes.get(&entry.task_id) {
                meta_line(
                    ui,
                    t,
                    d,
                    ph::GIT_MERGE,
                    t.warn,
                    &format!(
                        "补排覆盖了 {} 条没收口的批次：{}",
                        covered.len(),
                        covered.join("、")
                    ),
                );
            }

            // 阶段耗时：服务端回执里的 steps[]，按比例画成一条。老服务端缺席就不画。
            if let Some(steps) = entry.steps.as_deref().filter(|steps| !steps.is_empty()) {
                ui.add_space(d.px(space::S1));
                let total: u64 = steps.iter().map(|s| s.ms).sum();
                section_title(ui, t, d, &format!("阶段耗时 · 共 {} ms", group_ms(total)));
                stage_bar(ui, t, d, steps);
            }

            // 房间轮没有 batch：它的账在 `detail` 那格（面板 / 成员 / 死信）。
            if entry.kind == KIND_ROOM_RECALC
                && let Some(counts) = entry.detail
            {
                ui.add_space(d.px(space::S1));
                section_title(ui, t, d, "房间轮");
                meta_line(
                    ui,
                    t,
                    d,
                    ph::DOOR_OPEN,
                    t.text_secondary,
                    &format!(
                        "面板 {} · 成员 {} · 死信 {}",
                        counts.panels, counts.elements, counts.dead_letters
                    ),
                );
            }

            let units = outcome.map(|o| o.units.as_slice()).unwrap_or_default();
            if !units.is_empty() {
                ui.add_space(d.px(space::S1));
                section_title(ui, t, d, &format!("逐单元发布（{}）", units.len()));
                for unit in units {
                    unit_row(ui, t, d, unit, cmds);
                }
                let transformed = outcome.map_or(0, |o| o.transform_roots());
                if transformed > 0 {
                    sub_line(
                        ui,
                        t,
                        d,
                        &format!("其中 {transformed} 根走刚体前移便宜路（只动方位，网格未重算）"),
                        false,
                    );
                }
            }

            // 元素级变化清单（服务端 2026-10-24 W3）：服务端给了条数才画入口，清单本身按需取。
            if let Some(total) = entry.changes_total {
                ui.add_space(d.px(space::S1));
                changes_block(ui, t, d, entry, total, vm, cmds);
            }

            let warnings = outcome.map(|o| o.warnings.as_slice()).unwrap_or_default();
            if !warnings.is_empty() {
                ui.add_space(d.px(space::S1));
                warnings_block(ui, t, d, entry, warnings, state);
            }

            ui.add_space(d.px(space::S1));
            section_title(ui, t, d, "时间与标识");
            if let Some(times) = terminal_times(entry) {
                sub_line(ui, t, d, &times, false);
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = d.px(space::S1);
                ui.label(
                    RichText::new(&entry.task_id)
                        .font(Font::mono_micro(d))
                        .color(t.text_secondary),
                );
                let copy = ui.add(
                    egui::Label::new(
                        RichText::new(ph::COPY)
                            .font(Font::meta(d))
                            .color(t.text_muted),
                    )
                    .sense(Sense::click()),
                );
                if copy.on_hover_text("复制任务号").clicked() {
                    ui.ctx().copy_text(entry.task_id.clone());
                }
            });
        });
}

/// 「变化清单（N）」段：没取过给一个「加载」钮，在飞说在飞，失败给原文 + 重试；取回来一行一元素：
/// 操作丸标、refno、noun、动了哪些属性、折进了哪个根、账本去向；行尾「定位」。
fn changes_block(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    entry: &TaskEntry,
    total: u32,
    vm: &Vm,
    cmds: &mut Vec<Cmd>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = d.px(space::S2);
        section_title(ui, t, d, &format!("变化清单（{total}）"));
        match vm.changes.get(&entry.task_id) {
            None => {
                if ui
                    .add(
                        egui::Label::new(
                            RichText::new(format!("{} 加载清单", ph::LIST_BULLETS))
                                .font(Font::meta(d))
                                .color(t.accent),
                        )
                        .sense(Sense::click()),
                    )
                    .clicked()
                {
                    cmds.push(Cmd::LoadTaskChanges(entry.task_id.clone()));
                }
            }
            Some(ChangesState::Loading) => {
                ui.label(
                    RichText::new("加载中…")
                        .font(Font::meta(d))
                        .color(t.text_muted),
                );
            }
            Some(ChangesState::Failed(_)) => {
                if ui
                    .add(
                        egui::Label::new(
                            RichText::new(format!("{} 重试", ph::ARROW_CLOCKWISE))
                                .font(Font::meta(d))
                                .color(t.accent),
                        )
                        .sense(Sense::click()),
                    )
                    .clicked()
                {
                    cmds.push(Cmd::LoadTaskChanges(entry.task_id.clone()));
                }
            }
            Some(ChangesState::Loaded(_)) => {}
        }
    });
    match vm.changes.get(&entry.task_id) {
        Some(ChangesState::Failed(message)) => sub_line(ui, t, d, message, true),
        Some(ChangesState::Loaded(changes)) => {
            for change in &changes.items {
                change_row(ui, t, d, change, cmds);
            }
            if changes.truncated || changes.next_cursor.is_some() {
                sub_line(
                    ui,
                    t,
                    d,
                    &format!(
                        "只列了前 {} 条，共 {} 条",
                        changes.items.len(),
                        changes.total
                    ),
                    false,
                );
            }
        }
        _ => {}
    }
}

/// 服务端的 `op` → 丸标文案与色：增绿、删红、改蓝；改挂 / 换类型 / 成员变了是身份级的改，按警示色；认不出的原样显示。
fn change_op_label(op: &str) -> (String, Status) {
    match op {
        "added" => ("增".to_owned(), Status::Success),
        "deleted" => ("删".to_owned(), Status::Error),
        "modified" => ("改".to_owned(), Status::Info),
        "reparented" => ("改挂".to_owned(), Status::Warn),
        "type_changed" => ("换类型".to_owned(), Status::Warn),
        "members_changed" => ("成员".to_owned(), Status::Warn),
        "opaque" => ("字节变".to_owned(), Status::Neutral),
        "unresolved" => ("读不出".to_owned(), Status::Neutral),
        other => (other.to_owned(), Status::Neutral),
    }
}

/// 账本去向 → 芯片文案：`no_model` 带上哪一档放行的。
fn change_disposition_label(change: &ElementChange) -> Option<(String, Status)> {
    let disposition = change.disposition.as_deref()?;
    Some(match disposition {
        "rolled_up" => ("已上卷".to_owned(), Status::Neutral),
        "no_model" => {
            let reason = match change.no_model_reason.as_deref() {
                Some("metadata_only") => " · 只改元数据",
                Some("unread_attribute") => " · 生成器不读的属性",
                Some("cache_stamp") => " · 只跳了缓存戳",
                _ => "",
            };
            (format!("不影响模型{reason}"), Status::Neutral)
        }
        "unresolved" => ("未解析".to_owned(), Status::Warn),
        other => (other.to_owned(), Status::Neutral),
    })
}

fn change_row(ui: &mut Ui, t: &Tokens, d: Density, change: &ElementChange, cmds: &mut Vec<Cmd>) {
    let (op, status) = change_op_label(&change.op);
    let refno = RefU64::from_str(&change.refno)
        .ok()
        .filter(|r| r.is_valid());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = d.px(space::S2);
        ui.add(widgets::status_tag(t, d, &op, status));
        ui.label(
            RichText::new(&change.refno)
                .font(Font::mono_meta(d))
                .color(t.accent),
        );
        ui.label(
            RichText::new(&change.noun)
                .font(Font::meta_strong(d))
                .color(t.text_primary),
        );
        if !change.attrs.is_empty() {
            ui.add(
                egui::Label::new(
                    RichText::new(change.attrs.join(" · "))
                        .font(Font::mono_micro(d))
                        .color(t.text_secondary),
                )
                .truncate(),
            )
            .on_hover_text(change.details.join("\n"));
        }
        if let Some(root) = change.root_refno.as_deref() {
            if root != change.refno {
                ui.label(
                    RichText::new(format!("{} {root}", ph::ARROW_ELBOW_LEFT_UP))
                        .font(Font::mono_micro(d))
                        .color(t.text_muted),
                )
                .on_hover_text("折进的生成根");
            }
        }
        if let Some((label, status)) = change_disposition_label(change) {
            ui.add(widgets::status_tag(t, d, &label, status));
        }
        if let Some(refno) = refno {
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(
                        egui::Label::new(
                            RichText::new(format!("{} 定位", ph::CROSSHAIR))
                                .font(Font::meta(d))
                                .color(t.accent),
                        )
                        .sense(Sense::click()),
                    )
                    .clicked()
                {
                    cmds.push(Cmd::LocateElement(refno));
                }
            });
        }
    });
}

fn section_title(ui: &mut Ui, t: &Tokens, d: Density, title: &str) {
    ui.label(
        RichText::new(title)
            .font(Font::meta_strong(d))
            .color(t.text_secondary),
    );
}

/// 阶段耗时比例条 + 图例。每段至少 3 px，不然 2 ms 的 Ref0 快照就消失了；图例只列占比 ≥ 2% 的段，
/// 其余归「其它」。失败的段按红画——失败批次最后一段 `task_end ok=false` 就是这样被看到的。
fn stage_bar(ui: &mut Ui, t: &Tokens, d: Density, steps: &[TaskStep]) {
    let total: u64 = steps.iter().map(|s| s.ms).sum::<u64>().max(1);
    let w = ui.available_width().min(d.px(640.0));
    let h = d.px(8.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    if ui.is_rect_visible(rect) {
        let gap = d.px(1.5);
        let min_w = d.px(3.0);
        let usable = (w - gap * (steps.len().saturating_sub(1)) as f32).max(0.0);
        let mut x = rect.left();
        for step in steps {
            let seg = (usable * step.ms as f32 / total as f32).max(min_w);
            let seg_rect = Rect::from_min_size(egui::pos2(x, rect.top()), vec2(seg, h));
            let color = if !step.ok {
                t.danger
            } else {
                stage_color(t, &step.name)
            };
            ui.painter()
                .rect_filled(seg_rect, CornerRadius::same(2), color);
            x += seg + gap;
        }
    }
    let mut legend: Vec<String> = Vec::new();
    let mut other = 0u64;
    for step in steps {
        if step.ms * 50 >= total || !step.ok {
            legend.push(format!(
                "{} {}{}",
                stage_label(&step.name),
                group_ms(step.ms),
                if step.ok { "" } else { " ✗" }
            ));
        } else {
            other += step.ms;
        }
    }
    if other > 0 {
        legend.push(format!("其它 {}", group_ms(other)));
    }
    ui.label(
        RichText::new(legend.join(" · "))
            .font(Font::mono_micro(d))
            .color(t.text_muted),
    );
}

fn stage_color(t: &Tokens, name: &str) -> Color32 {
    match name {
        "collect_window" | "stage_apply" | "dependency_index" | "dependency_closure" => t.accent,
        "model_generate" => t.success,
        "durable_pending" | "write_marker" => t.warn,
        _ => t.border_strong,
    }
}

fn stage_label(name: &str) -> &str {
    match name {
        "data_parse" => "数据解析",
        "write_marker" => "写标记",
        "identity_check" => "身份复核",
        "resolve_window" => "解窗口",
        "collect_window" => "收集",
        "stage_apply" => "暂存应用",
        "dependency_index" => "依赖索引",
        "dependency_closure" => "依赖闭包",
        "model_generate" => "模型生成",
        "ref0_snapshot" => "Ref0 快照",
        "durable_pending" => "落库",
        "task_end" => "收尾",
        other => other,
    }
}

/// 毫秒数带千分位：1265 → 1,265。
fn group_ms(ms: u64) -> String {
    let digits = ms.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// 一个交付单元：状态图标 + noun + refno + 发布方式 + 消息；右侧三个动作把账本和三维视图连起来。
/// refno 解不出来就不给动作——契约给的是 PDMS 串，宁可少一排按钮也不去猜它想说什么。
fn unit_row(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    unit: &crate::model_update::UnitResult,
    cmds: &mut Vec<Cmd>,
) {
    let (icon, color) = match unit.status {
        UnitStatus::Generated => (ph::CHECK_CIRCLE, t.success),
        UnitStatus::Failed => (ph::X_CIRCLE, t.danger),
    };
    let kind = match unit.kind {
        UnitKind::Transform => "刚体前移",
        UnitKind::Regen => "整根重算",
    };
    let refno = RefU64::from_str(&unit.root_refno)
        .ok()
        .filter(|r| r.is_valid());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = d.px(space::S2);
        ui.label(RichText::new(icon).font(Font::meta(d)).color(color));
        ui.label(
            RichText::new(&unit.noun)
                .font(Font::meta_strong(d))
                .color(t.text_primary),
        );
        ui.label(
            RichText::new(&unit.root_refno)
                .font(Font::mono_meta(d))
                .color(t.accent),
        );
        ui.add(widgets::status_tag(t, d, kind, Status::Neutral));
        // 房间前 → 后（服务端 2026-10-24 W1）：两格都缺席就是没拍，不画；拍了没变只写一个。
        if let Some((before, after)) = room_change(unit) {
            let changed = before != after;
            let text = if changed {
                format!("房间 {before} → {after}")
            } else {
                format!("房间 {after}")
            };
            ui.add(widgets::status_tag(
                t,
                d,
                &text,
                if changed {
                    Status::Info
                } else {
                    Status::Neutral
                },
            ))
            .on_hover_text(room_change_tooltip(unit));
        }
        // 几何前 → 后（服务端 2026-10-24 W4）：服务端按构件对好的差异，没拍不画；悬停给包围盒和样本 refno。
        if let Some(diff) = unit.geometry_diff.as_ref() {
            let (text, status) = if diff.moved + diff.added + diff.removed == 0 {
                ("几何未动".to_owned(), Status::Neutral)
            } else {
                let mut parts = Vec::new();
                if diff.moved > 0 {
                    parts.push(format!("动 {}", diff.moved));
                }
                if diff.added > 0 {
                    parts.push(format!("多 {}", diff.added));
                }
                if diff.removed > 0 {
                    parts.push(format!("少 {}", diff.removed));
                }
                (format!("几何 {}", parts.join(" · ")), Status::Info)
            };
            ui.add(widgets::status_tag(t, d, &text, status))
                .on_hover_text(geometry_tooltip(unit));
        }
        if let Some(message) = unit.message.as_deref() {
            ui.add(
                egui::Label::new(RichText::new(message).font(Font::meta(d)).color(
                    if unit.status == UnitStatus::Failed {
                        t.danger
                    } else {
                        t.text_muted
                    },
                ))
                .truncate(),
            );
        }
        if let Some(refno) = refno {
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = d.px(space::S3);
                let action = |ui: &mut Ui, icon: &str, label: &str| -> bool {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("{icon} {label}"))
                                .font(Font::meta(d))
                                .color(t.accent),
                        )
                        .sense(Sense::click()),
                    )
                    .clicked()
                };
                if action(ui, ph::DOOR_OPEN, "房间") {
                    cmds.push(Cmd::ShowRooms(refno));
                }
                if action(ui, ph::EYE, "显示") {
                    cmds.push(Cmd::Model(ModelAction::SetVisible {
                        refnos: vec![refno],
                        visible: true,
                    }));
                }
                if action(ui, ph::CROSSHAIR, "定位") {
                    cmds.push(Cmd::LocateElement(refno));
                }
            });
        }
    });
}

/// 单元的房间前后各折成一串房间号（去重、排序；空 = 「—」）。服务端没拍（两格都 `None`）就 `None`。
fn room_change(unit: &crate::model_update::UnitResult) -> Option<(String, String)> {
    let (Some(before), Some(after)) = (unit.rooms_before.as_ref(), unit.rooms_after.as_ref())
    else {
        return None;
    };
    let fold = |rooms: &[crate::model_update::RoomRef]| {
        let mut names: Vec<&str> = rooms
            .iter()
            .map(|room| room.room_num.as_str())
            .filter(|name| !name.is_empty())
            .collect();
        names.sort_unstable();
        names.dedup();
        if names.is_empty() {
            "—".to_owned()
        } else {
            names.join(", ")
        }
    };
    Some((fold(before), fold(after)))
}

/// 悬停里给到面板和构件数：`R113 · 面板 24381_1204 · 2 个构件`。
fn room_change_tooltip(unit: &crate::model_update::UnitResult) -> String {
    let list = |title: &str, rooms: Option<&Vec<crate::model_update::RoomRef>>| {
        let rooms = rooms.map(Vec::as_slice).unwrap_or_default();
        if rooms.is_empty() {
            return format!("{title}：无房间归属");
        }
        let lines: Vec<String> = rooms
            .iter()
            .map(|room| {
                format!(
                    "  {} · 面板 {} · {} 个构件",
                    if room.room_num.is_empty() {
                        "(无房间号)"
                    } else {
                        room.room_num.as_str()
                    },
                    room.panel_refno,
                    room.members
                )
            })
            .collect();
        format!("{title}：\n{}", lines.join("\n"))
    };
    format!(
        "{}\n{}",
        list("开工前", unit.rooms_before.as_ref()),
        list("收尾后", unit.rooms_after.as_ref())
    )
}

/// 几何芯片的悬停：前后并集包围盒（每轴只在动了的轴上写差）、行数、样本 refno。
fn geometry_tooltip(unit: &crate::model_update::UnitResult) -> String {
    let mut lines = Vec::new();
    if let (Some(before), Some(after)) =
        (unit.geometry_before.as_ref(), unit.geometry_after.as_ref())
    {
        lines.push(format!("模型行 {} → {}", before.records, after.records));
        match (before.aabb, after.aabb) {
            (Some(b), Some(a)) => {
                for (axis, i) in [("x", 0usize), ("y", 1), ("z", 2)] {
                    let (b0, b1, a0, a1) = (b[i], b[i + 3], a[i], a[i + 3]);
                    if (b0 - a0).abs() > 0.5 || (b1 - a1).abs() > 0.5 {
                        lines.push(format!(
                            "包围盒 {axis}: {b0:.0}..{b1:.0} → {a0:.0}..{a1:.0}"
                        ));
                    }
                }
                if lines.len() == 1 {
                    lines.push("包围盒未变".to_owned());
                }
            }
            (None, Some(a)) => lines.push(format!("包围盒：无 → z {:.0}..{:.0}", a[2], a[5])),
            (Some(b), None) => lines.push(format!("包围盒：z {:.0}..{:.0} → 无", b[2], b[5])),
            (None, None) => {}
        }
    }
    if let Some(diff) = unit.geometry_diff.as_ref()
        && !diff.sample.is_empty()
    {
        lines.push(format!("有变化的构件：{}", diff.sample.join(", ")));
    }
    lines.join("\n")
}

/// 告警段：默认折起，头一行是条数 + 每条告警的标签（「 — 」或「：」之前那半句）摘要；点开才列原文。
fn warnings_block(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    entry: &TaskEntry,
    warnings: &[String],
    state: &mut State,
) {
    let open = state.warnings_open.contains(&entry.task_id);
    let head = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = d.px(space::S2);
        ui.label(RichText::new(ph::WARNING).font(Font::meta(d)).color(t.warn));
        ui.label(
            RichText::new(format!("告警 {} 条", warnings.len()))
                .font(Font::meta_strong(d))
                .color(t.text_secondary),
        );
        ui.label(
            RichText::new(if open { ph::CARET_UP } else { ph::CARET_RIGHT })
                .font(Font::meta(d))
                .color(t.text_muted),
        );
        if !open {
            // 折叠态只摆标签摘要，摆不下的截成省略号——十来条告警的标签串起来比行宽长。
            let tags: Vec<&str> = warnings.iter().map(|w| warning_tag(w)).collect();
            ui.add(
                egui::Label::new(
                    RichText::new(tags.join(" · "))
                        .font(Font::meta(d))
                        .color(t.text_muted),
                )
                .truncate(),
            );
        }
    });
    if ui
        .interact(
            head.response.rect,
            egui::Id::new(("incr-warn", &entry.task_id)),
            Sense::click(),
        )
        .clicked()
        && !state.warnings_open.remove(&entry.task_id)
    {
        state.warnings_open.insert(entry.task_id.clone());
    }
    if open {
        for warning in warnings {
            ui.label(
                RichText::new(warning)
                    .font(Font::mono_micro(d))
                    .color(t.text_muted),
            );
        }
    }
}

/// 告警原文的标签：去掉开头的 `dbnum=N:` 前缀，取第一个「 — 」/「：」/「: 」/「 (」之前
/// 的那半句，最多 24 个字——折叠态一行里要摆得下好几条。
fn warning_tag(warning: &str) -> &str {
    let mut text = warning.trim();
    loop {
        let Some(rest) = text.strip_prefix("dbnum=") else {
            break;
        };
        let Some(colon) = rest.find(':') else {
            break;
        };
        text = rest[colon + 1..].trim_start();
    }
    let cut = [" — ", "：", ": ", "——", " (", "（"]
        .iter()
        .filter_map(|sep| text.find(*sep))
        .min()
        .unwrap_or(text.len());
    let head = text[..cut].trim_end();
    match head.char_indices().nth(24) {
        Some((idx, _)) => head[..idx].trim_end(),
        None => head,
    }
}

/// 任务表这一格单独失败时的细条提示——版面不整页让位，下面的旧快照还在，
/// 条幅要把「这是旧账」说在最顶上。
fn degraded_banner(ui: &mut Ui, t: &Tokens, d: Density, text: &str) {
    let (_, rect) = ui.allocate_space(vec2(ui.available_width(), d.px(26.0)));
    ui.painter().rect_filled(rect, 0.0, t.warn_bg);
    ui.scope_builder(
        egui::UiBuilder::new()
            .layout(egui::Layout::left_to_right(Align::Center))
            .max_rect(rect.shrink2(vec2(d.px(14.0), 0.0)))
            .id(egui::Id::new("incr-banner")),
        |ui| {
            ui.spacing_mut().item_spacing.x = d.px(space::S2);
            ui.label(RichText::new(ph::WARNING).font(Font::meta(d)).color(t.warn));
            ui.label(
                RichText::new(text)
                    .font(Font::meta(d))
                    .color(t.text_secondary),
            );
        },
    );
}

/// `room_rebuild` 的 `trigger` 译成人话；认不出的原样显示。
fn trigger_label(trigger: &str) -> &str {
    match trigger {
        "startup_alignment" => "启动对齐",
        "startup_full_init" => "启动完整初始化",
        "rooms_rebuild" => "重新生成房间",
        other => other,
    }
}

fn kind_glyph(t: &Tokens, entry: &TaskEntry) -> (&'static str, egui::Color32, &'static str) {
    match entry.kind.as_str() {
        KIND_DATA_BATCH => (ph::GIT_COMMIT, t.accent, "数据批次"),
        KIND_ROOM_RECALC => (ph::DOOR_OPEN, t.accent, "房间重算"),
        KIND_ROOM_REBUILD => (ph::DOOR_OPEN, t.accent, "整库房间重算"),
        KIND_MODEL_REBUILD => (ph::CUBE, t.accent, "模型重建"),
        KIND_MODEL_DRAIN => (ph::ARROWS_CLOCKWISE, t.text_muted, "模型排水"),
        _ => (ph::FILE, t.text_muted, "任务"),
    }
}

fn state_chip(entry: &TaskEntry) -> (&'static str, Status) {
    match entry.state.as_str() {
        "succeeded" => ("成功", Status::Success),
        "partial" => ("部分成功", Status::Warn),
        "failed" => ("失败", Status::Error),
        "yielded" => ("已让位", Status::Neutral),
        "queued" => ("排队中", Status::Neutral),
        "running" => ("运行中", Status::Info),
        _ => ("未知", Status::Neutral),
    }
}

/// 变化摘要行：`增 2 · 改 1 · 删 0` 或退回总数一句。三数缺席时不画——`change_breakdown`
/// 的 None 判据与队列页同一套（缺字段与真零不可分，都不能画成「无变化」）。
fn change_summary(entry: &TaskEntry) -> String {
    let Some(batch) = entry.result.as_ref().and_then(|o| o.batch.as_ref()) else {
        return String::new();
    };
    match batch.change_breakdown() {
        Some(b) => format!("增 {} · 改 {} · 删 {}", b.added, b.modified, b.deleted),
        None if batch.changed_elements > 0 => format!("共 {} 项变化", batch.changed_elements),
        None => String::new(),
    }
}

/// 明细行：图标 + 一句话。配色由调用方定——「这是什么」与「它怎么了」
/// 未必共用一个语气。
fn meta_line(ui: &mut Ui, t: &Tokens, d: Density, icon: &str, color: egui::Color32, text: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon).font(Font::meta(d)).color(color));
        ui.label(
            RichText::new(text)
                .font(Font::meta(d))
                .color(t.text_primary),
        );
    });
}

/// 次说明行：消息正文、并入清单这类「挂在上一行下面」的补充文字。
fn sub_line(ui: &mut Ui, t: &Tokens, d: Density, text: &str, warn: bool) {
    let color = if warn { t.danger } else { t.text_muted };
    ui.label(RichText::new(text).font(Font::meta(d)).color(color));
}

/// 这批次在时间轴上的锚点：保存窗口右端的写入时刻；没有（房间轮 / 老服务端）就用入队时刻。
fn anchor(entry: &TaskEntry) -> Option<DateTime<Local>> {
    entry
        .end_sesno_time
        .as_deref()
        .and_then(parse)
        .or_else(|| parse(&entry.created_at))
}

/// 主行上的保存时刻：窗口两端同一分钟就只写一个。两端都没有就不写，不回落成 sesno。
fn saved_at(entry: &TaskEntry) -> Option<String> {
    let end = parse(entry.end_sesno_time.as_deref()?)?;
    let start = entry.start_sesno_time.as_deref().and_then(parse);
    let today = Local::now().date_naive();
    let fmt = |at: DateTime<Local>| {
        if at.date_naive() == today {
            at.format("%H:%M:%S").to_string()
        } else {
            at.format("%m-%d %H:%M").to_string()
        }
    };
    Some(match start {
        Some(start)
            if start.format("%m-%d %H:%M").to_string() != end.format("%m-%d %H:%M").to_string() =>
        {
            format!("{} → {} 保存", fmt(start), fmt(end))
        }
        _ => format!("{} 保存", fmt(end)),
    })
}

/// 并入保存的时刻清单，与队列页同一规则：一条解不出来就整列不给。
fn save_list(times: &[&str]) -> Option<String> {
    let parsed: Vec<DateTime<Local>> = times.iter().map(|at| parse(at)).collect::<Option<_>>()?;
    if parsed.is_empty() {
        return None;
    }
    let same_day = parsed
        .windows(2)
        .all(|pair| pair[0].date_naive() == pair[1].date_naive());
    Some(
        parsed
            .iter()
            .enumerate()
            .map(|(i, at)| {
                if same_day && i > 0 {
                    at.format("%H:%M").to_string()
                } else {
                    at.format("%m-%d %H:%M").to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" / "),
    )
}

/// 起止时刻配成一句 `入队 10-16 01:36 · 开跑 01:37 · 耗时 01:02`；起不全就不说这句。
fn terminal_times(entry: &TaskEntry) -> Option<String> {
    let started = parse(entry.started_at.as_deref()?)?;
    let queued = parse(&entry.created_at)?;
    let mut parts = vec![
        format!("入队 {}", queued.format("%m-%d %H:%M:%S")),
        format!("开跑 {}", started.format("%H:%M:%S")),
    ];
    if let Some(finished) = entry.finished_at.as_deref().and_then(parse) {
        parts.push(format!("完成 {}", finished.format("%H:%M:%S")));
        let elapsed = finished.signed_duration_since(started).to_std().ok()?;
        parts.push(format!("耗时 {}", clock(elapsed)));
    }
    Some(parts.join(" · "))
}

fn elapsed_of(entry: &TaskEntry) -> Option<std::time::Duration> {
    let started = parse(entry.started_at.as_deref()?)?;
    let end = entry
        .finished_at
        .as_deref()
        .and_then(parse)
        .unwrap_or_else(Local::now);
    end.signed_duration_since(started).to_std().ok()
}

fn clock(elapsed: std::time::Duration) -> String {
    let s = elapsed.as_secs();
    if s < 3600 {
        format!("{:02}:{:02}", s / 60, s % 60)
    } else {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    }
}

fn parse(stamp: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(stamp)
        .ok()
        .map(|at| at.with_timezone(&Local))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_update::{BatchResult, Outcome, RunStatus};
    use crate::task_queue::TaskList;

    /// `kind` 筛的口径钉死：数据批次 / 房间轮 / 整库重建各归各桶，别的串不进
    /// 任何一个窄筛——「全部」是唯一兑底桶。
    #[test]
    fn filter_buckets_by_server_kind() {
        assert!(Filter::DataBatch.matches("data_batch"));
        assert!(Filter::Room.matches("room_recalc"));
        assert!(Filter::ModelRebuild.matches("model_rebuild"));
        assert!(!Filter::DataBatch.matches("room_recalc"));
        assert!(!Filter::Room.matches("data_batch"));
        assert!(Filter::All.matches("anything"));
        assert!(StateFilter::Failed.matches("partial"));
        assert!(!StateFilter::Succeeded.matches("running"));
        assert!(StateFilter::Active.matches("queued"));
    }

    /// 本批次的真实回执形状（verification/incr-pipe-room-20261023 那一轮）必须
    /// 解得出：单元明细、批次判定、并入清单、阶段耗时一个字段都不能丢。
    #[test]
    fn real_8023_task_entry_decodes() {
        let raw = r#"{"tasks":[{"task_id":"db-20261023-131254-000008","kind":"data_batch","state":"succeeded",
            "project":"AvevaMarineSample","created_at":"2026-10-23T13:12:54+08:00",
            "started_at":"2026-10-23T13:12:57+08:00","finished_at":"2026-10-23T13:13:02+08:00",
            "dbnum":8000,"db_type":"DESI","start_sesno":658,"end_sesno":659,
            "start_sesno_time":"2026-10-23T05:12:48+00:00","end_sesno_time":"2026-10-23T05:12:48+00:00",
            "units_done":1,"total_units":1,"events_seen":6,
            "detail":{"intent":"apply_window","frozen_target_sesno":659,"stages":["data","generation"]},
            "result":{"status":"success","batch":{"dbnum":8000,"start_sesno":658,"end_sesno":659,
                "status":"applied","merged_sesnos":[658,659],"changed_elements":3,
                "added_elements":0,"modified_elements":3,"deleted_elements":0},
                "units":[{"dbnum":8000,"root_refno":"24384/24776","noun":"EQUI","status":"generated","kind":"regen"}],
                "warnings":["dbnum=8000: rigid-motion (model_transform_cheap_path) — transform=0 rejected=1"]},
            "steps":[{"name":"data_parse","at":"x","ms":33,"ok":true},{"name":"stage_apply","at":"x","ms":1265,"ok":true},
                {"name":"model_generate","at":"x","ms":333,"ok":true},{"name":"task_end","at":"x","ms":29,"ok":true}]}]}"#;
        let list: TaskList = serde_json::from_str(raw).expect("task list decodes");
        let entry = &list.tasks[0];
        let outcome = entry.result.as_ref().expect("result present");
        assert_eq!(outcome.units[0].kind, UnitKind::Regen);
        assert_eq!(state_chip(entry).0, "成功");
        let steps = entry.steps.as_deref().expect("steps decode");
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[1].name, "stage_apply");
        assert_eq!(steps[1].ms, 1265);
        assert!(steps[3].ok);
        assert_eq!(warning_tag(&outcome.warnings[0]), "rigid-motion");
        assert_eq!(
            warning_tag("dbnum=7998: 14 个变更无法解析合法生成根，跳过模型生成"),
            "14 个变更无法解析合法生成根，跳过模型生成"
        );
        assert_eq!(
            warning_tag("目录反查退化 — 种子 6 → 闭包 6"),
            "目录反查退化"
        );
        assert!(matches_query(entry, "24384/24776"));
        assert!(matches_query(entry, "24384_247"));
        assert!(matches_query(entry, "equi"));
        assert!(matches_query(entry, "659"));
        assert!(!matches_query(entry, "7999"));
        assert!(has_changes(entry));
    }

    /// 三数全零与字段缺席都回 `None`——记录页同样不许把 0/0/0 画成「无变化」。
    #[test]
    fn change_summary_stays_silent_without_breakdown() {
        let mut entry = TaskEntry::default();
        entry.kind = KIND_DATA_BATCH.into();
        entry.result = Some(Outcome {
            status: RunStatus::Success,
            batch: Some(BatchResult {
                dbnum: 8000,
                status: BatchStatus::Applied,
                changed_elements: 5,
                ..Default::default()
            }),
            ..Default::default()
        });
        assert_eq!(change_summary(&entry), "共 5 项变化");
    }

    /// 补排链的推断：2026-10-23 实测的那对——654→654 失败（E3D 还在写），5 s 后 654→655 成功覆盖它。
    /// 别的库、更早入队的、窗口没盖住右端的都不算。
    #[test]
    fn supersession_links_the_refused_batch_to_its_follow_up() {
        let row =
            |id: &str, dbnum: u32, state: &str, start: i32, end: i32, created: &str| TaskEntry {
                task_id: id.into(),
                kind: KIND_DATA_BATCH.into(),
                state: state.into(),
                dbnum: Some(dbnum),
                start_sesno: Some(start),
                end_sesno: Some(end),
                created_at: created.into(),
                ..Default::default()
            };
        let failed = row(
            "db-2",
            8000,
            "failed",
            654,
            654,
            "2026-10-23T12:53:24+08:00",
        );
        let follow = row(
            "db-4",
            8000,
            "succeeded",
            654,
            655,
            "2026-10-23T12:53:29+08:00",
        );
        let other_db = row(
            "db-5",
            7998,
            "succeeded",
            654,
            655,
            "2026-10-23T12:53:29+08:00",
        );
        let earlier = row(
            "db-1",
            8000,
            "succeeded",
            652,
            653,
            "2026-10-23T12:12:54+08:00",
        );
        let later_miss = row(
            "db-6",
            8000,
            "succeeded",
            656,
            657,
            "2026-10-23T13:06:24+08:00",
        );
        let entries = [&later_miss, &follow, &other_db, &failed, &earlier];
        let linked = chains(&entries);
        assert_eq!(
            linked.superseded_by.get("db-2").map(String::as_str),
            Some("db-4")
        );
        assert_eq!(
            linked.supersedes.get("db-4"),
            Some(&vec!["db-2".to_string()])
        );
        assert!(!linked.superseded_by.contains_key("db-1"));
        assert!(
            linked.inferred.contains("db-2"),
            "老服务端没字段，这条是推的"
        );

        // partial 是数据已应用、下一窗不重覆盖：不推（与服务端同一口径）。
        let partial = row(
            "db-7",
            8000,
            "partial",
            658,
            659,
            "2026-10-23T13:12:54+08:00",
        );
        let after = row(
            "db-8",
            8000,
            "succeeded",
            658,
            660,
            "2026-10-23T13:20:00+08:00",
        );
        assert!(chains(&[&partial, &after]).superseded_by.is_empty());
    }

    /// 服务端给了 `superseded_by` / `supersedes`（gen-model 2026-10-24）就读字段：窗口对得上也不再猜
    /// 别的失败批次，`inferred` 空；两边字段只给一半也能拼成整条链。
    #[test]
    fn server_links_win_over_window_inference() {
        let row = |id: &str, state: &str, start: i32, end: i32, created: &str| TaskEntry {
            task_id: id.into(),
            kind: KIND_DATA_BATCH.into(),
            state: state.into(),
            dbnum: Some(8000),
            start_sesno: Some(start),
            end_sesno: Some(end),
            created_at: created.into(),
            ..Default::default()
        };
        let mut failed = row("db-2", "failed", 654, 654, "2026-10-23T12:53:24+08:00");
        failed.superseded_by = Some("db-4".into());
        let follow = row("db-4", "succeeded", 654, 655, "2026-10-23T12:53:29+08:00");
        // 窗口上看像被 db-9 补排、但服务端没连：不猜。
        let orphan = row("db-8", "failed", 660, 660, "2026-10-23T14:00:00+08:00");
        let later = row("db-9", "succeeded", 660, 661, "2026-10-23T14:00:05+08:00");
        let linked = chains(&[&later, &orphan, &follow, &failed]);
        assert_eq!(
            linked.superseded_by.get("db-2").map(String::as_str),
            Some("db-4")
        );
        assert_eq!(
            linked.supersedes.get("db-4"),
            Some(&vec!["db-2".to_string()])
        );
        assert!(!linked.superseded_by.contains_key("db-8"));
        assert!(linked.inferred.is_empty());

        // 只有后继那边带 `supersedes` 也够。
        let mut follow_only = follow.clone();
        follow_only.supersedes = vec!["db-2".into()];
        let plain_failed = row("db-2", "failed", 654, 654, "2026-10-23T12:53:24+08:00");
        let linked = chains(&[&follow_only, &plain_failed]);
        assert_eq!(
            linked.superseded_by.get("db-2").map(String::as_str),
            Some("db-4")
        );

        // 两边都带字段、后继排在前面（轮询页 + 翻回来的页拼在一起就是这个序）：同一条链只算一次。
        let linked = chains(&[&follow_only, &failed]);
        assert_eq!(
            linked.supersedes.get("db-4"),
            Some(&vec!["db-2".to_string()])
        );
        assert_eq!(trigger_label("rooms_rebuild"), "重新生成房间");
        assert_eq!(trigger_label("something_new"), "something_new");
    }

    /// 单元的房间前后（服务端 W1）：两格都有才画；房间号去重排序；空是「—」；老回执缺席解成 `None`。
    #[test]
    fn a_unit_room_change_folds_to_room_numbers_or_nothing() {
        use crate::model_update::{RoomRef, UnitResult};
        let room = |num: &str, panel: &str, members: u32| RoomRef {
            room_num: num.into(),
            panel_refno: panel.into(),
            members,
        };
        let mut unit = UnitResult {
            root_refno: "24384/23257".into(),
            ..Default::default()
        };
        assert_eq!(room_change(&unit), None, "没拍就不画");
        unit.rooms_before = Some(vec![]);
        unit.rooms_after = Some(vec![
            room("R345", "24381_35186", 1),
            room("R113", "24381_1204", 2),
        ]);
        assert_eq!(room_change(&unit), Some(("—".into(), "R113, R345".into())));
        let tip = room_change_tooltip(&unit);
        assert!(
            tip.contains("开工前：无房间归属")
                && tip.contains("R113 · 面板 24381_1204 · 2 个构件"),
            "{tip}"
        );

        let decoded: UnitResult = serde_json::from_str(
            r#"{"dbnum":8000,"root_refno":"24384/23257","noun":"BRAN","status":"generated",
                "rooms_before":[],"rooms_after":[{"room_num":"R113","panel_refno":"24381_1204","members":2}]}"#,
        )
        .unwrap();
        assert_eq!(room_change(&decoded), Some(("—".into(), "R113".into())));
        let legacy: UnitResult = serde_json::from_str(
            r#"{"dbnum":8000,"root_refno":"24384/23257","noun":"BRAN","status":"generated"}"#,
        )
        .unwrap();
        assert_eq!(room_change(&legacy), None);
        assert!(legacy.geometry_diff.is_none() && geometry_tooltip(&legacy).is_empty());

        // 几何前后（服务端 W4）：只在动了的轴上写包围盒差，样本 refno 列出来。
        let moved: UnitResult = serde_json::from_str(
            r#"{"dbnum":8000,"root_refno":"24384/23257","noun":"BRAN","status":"generated",
                "geometry_before":{"records":9,"aabb":[10867.9,11979.0,2900.0,11251.4,12352.0,3480.0]},
                "geometry_after":{"records":9,"aabb":[10867.9,11979.0,2900.0,11251.4,12352.0,3980.0]},
                "geometry_diff":{"moved":1,"added":0,"removed":0,"sample":["24384_23262"]}}"#,
        )
        .unwrap();
        let tip = geometry_tooltip(&moved);
        assert!(tip.contains("模型行 9 → 9"), "{tip}");
        assert!(tip.contains("包围盒 z: 2900..3480 → 2900..3980"), "{tip}");
        assert!(!tip.contains("包围盒 x"), "没动的轴不写：{tip}");
        assert!(tip.contains("24384_23262"), "{tip}");
    }

    /// 日期分组按保存窗口右端的写入时刻，没有就按入队时刻；都没有就是「时间未知」。
    #[test]
    fn day_anchor_prefers_the_save_time() {
        let mut entry = TaskEntry::default();
        entry.created_at = "2026-10-23T13:12:54+08:00".into();
        entry.end_sesno_time = Some("2026-10-22T17:12:48+00:00".into());
        let at = anchor(&entry).expect("anchor");
        assert_eq!(
            at.date_naive(),
            parse("2026-10-22T17:12:48+00:00").unwrap().date_naive()
        );
        entry.end_sesno_time = None;
        assert!(anchor(&entry).is_some());
        entry.created_at.clear();
        assert!(anchor(&entry).is_none());
        assert_eq!(day_label(None), "时间未知");
        assert_eq!(group_ms(1265), "1,265");
        assert_eq!(group_ms(33), "33");
        assert_eq!(group_ms(1_000_000), "1,000,000");
    }
}
