//! 「重新生成模型」/「清理模型」共用的确认框（ADR-0030 D2；2026-10-10 清理模型计划 D2）。
//!
//! 这一步不发任何请求，它存在的唯一理由和模型更新的确认步一样：**按下去的是
//! 一个收不回来的按钮**——范围内已有的产物会被服务端重算替换（重新生成），或被服务端
//! 删掉（清理）。两件会改模型库的事只有这一个框：文案按 [`Kind`] 查表，「跑起来之后
//! 没有回头路」的纪律两种共用。确认框不带数字：客户端不清点、不解根、不删产物，
//! 解根 / 删旧 / 重算全在服务端那一发 `POST /model/generate { force: true }` 或
//! `DELETE /model/subtree` 里；此前「清点 → 数字确认 → 整片删 → 逐根调 → 停在这里」
//! 那条流水随 ADR-0030 退役。

use egui::RichText;

use crate::Cmd;
use crate::style::tokens::{Density, Status, Tokens};
use crate::style::widgets;

/// 这一框在问哪一件事。几件都改模型库 / 房间数据，差别只在文案与按下去之后服务端做什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 右键「重新生成模型」：服务端先删后生成，跑完显示。
    Regenerate,
    /// 右键「清理模型」：服务端只删产物、不重算；跑完把三维里对应的模型卸掉。
    Cleanup,
    /// 顶栏「重新生成房间」（全库）：服务端清掉全部房间归属数据、换代重建常驻
    /// 参照模型、逐库整库重算房间关系。日常改动走增量，这是全量重置入口。
    RoomRebuild,
}

impl Kind {
    /// 窗标题，也是菜单上那一项的名字。
    pub fn title(self) -> &'static str {
        match self {
            Self::Regenerate => "重新生成模型",
            Self::Cleanup => "清理模型",
            Self::RoomRebuild => "重新生成房间",
        }
    }

    /// 确认按钮上的动词。
    fn confirm_label(self) -> &'static str {
        match self {
            Self::Regenerate => "重新生成",
            Self::Cleanup => "清理",
            Self::RoomRebuild => "重新生成房间",
        }
    }

    fn confirm_icon(self) -> &'static str {
        match self {
            Self::Regenerate => egui_phosphor::regular::ARROWS_CLOCKWISE,
            Self::Cleanup => egui_phosphor::regular::BROOM,
            Self::RoomRebuild => egui_phosphor::regular::HOUSE,
        }
    }

    /// 正文下面那行小字：这一按之后服务端会做什么、人该等什么。
    fn note(self) -> &'static str {
        match self {
            Self::Regenerate => {
                "服务端自己解根、重算；跑完会把这个范围的模型显示出来。大范围可能超过服务端 \
                 120 秒预算，那时服务端后台继续，完成后再点眼睛查看。"
            }
            Self::Cleanup => {
                "只删已生成的产物，不删设计元素；服务端按子树删，范围内有根 \
                 正在生成会被拒绝（稍后再试）。跑完三维里对应的模型会卸载。"
            }
            Self::RoomRebuild => {
                "日常增删改走增量更新，不需要点它；这是「房间数据不对了」的全量重置。 \
                 重建期间的普通生成请求会被服务端拒绝（409），稍后再试。"
            }
        }
    }

    fn running_line(self, label: &str) -> String {
        match self {
            Self::Regenerate => format!("正在生成 {label} 的模型…"),
            Self::Cleanup => format!("正在清理 {label} 的模型产物…"),
            Self::RoomRebuild => format!("正在重建 {label} 的房间关系…"),
        }
    }

    fn running_note(self) -> &'static str {
        match self {
            Self::Regenerate => "服务端正在重算，停不下来；跑完自动显示。",
            Self::Cleanup => "服务端正在删除，停不下来；跑完自动卸载对应的模型。",
            Self::RoomRebuild => "服务端正在重建，停不下来；跑完进度屏自己收摊。",
        }
    }

    fn failed_verb(self) -> &'static str {
        match self {
            Self::Regenerate => "生成失败",
            Self::Cleanup => "清理失败",
            Self::RoomRebuild => "重建失败",
        }
    }
}

/// 确认框此刻在说什么。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Vm {
    /// 等人按。还什么都没发。`dbnum` 有值时这一框问的是整库清理，不是右键那几行。
    ///
    /// `purge_meshes` 是「一并删除网格文件」那一格此刻的状态。两个清理档都画这一格
    /// （右键「清理模型」走子树、整库清理走整库，两条服务端接口都收这个参数）；重新生成
    /// 不带这一格——它走 `force = true`，服务端先删后算，网格删了就得整批重写一遍。
    Confirm {
        kind: Kind,
        label: String,
        dbnum: Option<u32>,
        purge_meshes: bool,
    },
    /// 等房间参照模型装载 / 重建完。这一档今天只有「重新生成房间」用到：重建请求
    /// 已经发出、服务端收不回，所以按钮是「后台继续」——只摘进度屏，服务端照跑到底。
    ///
    /// `phase` 是服务端进度的阶段字面（`spatial-tree` / `source-snapshot` / `mesh-warm`
    /// / `edge-recompute`）；`files` / `meshes` / `dbnums` 是 `(done, total[, failed])`，
    /// 老服务端没有 `progress` 键时为 `None`——界面退化成 spinner + 阶段名。
    WaitingResident {
        kind: Kind,
        label: String,
        phase: String,
        files: Option<(usize, usize)>,
        meshes: Option<(usize, usize, usize)>,
        dbnums: Option<(usize, usize)>,
    },
    /// 那一发在途。**这一档没有回头路**，所以窗口右上角那个叉也不给：服务端是
    /// `await_background_without_cancelling`（生成）/ 同步删到底（清理），关窗关不掉任何东西。
    Running { kind: Kind, label: String },
    /// 失败。服务端那边没做成（或做了一半由它自己收拾），这里只有关闭。
    Failed {
        kind: Kind,
        label: String,
        reason: String,
    },
}

impl Vm {
    /// 窗上写的那个范围（右键落点的显示名，多选时形如 `/VESSEL-01 等 3 项`）。
    pub fn label(&self) -> &str {
        match self {
            Self::Confirm { label, .. }
            | Self::WaitingResident { label, .. }
            | Self::Running { label, .. }
            | Self::Failed { label, .. } => label,
        }
    }

    pub fn kind(&self) -> Kind {
        match self {
            Self::Confirm { kind, .. }
            | Self::WaitingResident { kind, .. }
            | Self::Running { kind, .. }
            | Self::Failed { kind, .. } => *kind,
        }
    }
}

/// 确认框正文。不带数字：数字要靠清点，而清点那条路已经退役；人要判断的只有
/// 「这个范围要不要重算 / 清掉」。
/// 库行「清理模型（整库）」的正文。与右键那一句不是同一件事：这一按清的是整个库的模型面。
pub fn dbnum_cleanup_prompt(dbnum: u32) -> String {
    format!(
        "清理 db{dbnum} 的全部模型？该库已生成的模型产物会被删除、模型水位归零，三维里该库的模型会卸载；之后请点「重算模型（整库）」重新生成。"
    )
}

pub fn confirm_prompt(kind: Kind, label: &str) -> String {
    match kind {
        Kind::Regenerate => format!("对 {label} 重新生成模型？范围内已有的产物会被重算替换。"),
        Kind::Cleanup => format!(
            "对 {label} 清理模型？范围内已生成的产物会被删除，三维里对应的模型会卸载；需要时再右键「重新生成模型」。"
        ),
        Kind::RoomRebuild => {
            "重新生成全部房间关系？服务端会删除两张房间边表与停放链、换代重建常驻参照模型，再对每个库整库重算房间关系。"
                .to_owned()
        }
    }
}

pub fn show(ctx: &egui::Context, t: &Tokens, d: Density, vm: Option<&mut Vm>) -> Vec<Cmd> {
    let Some(vm) = vm else {
        return Vec::new();
    };

    let mut cmds = Vec::new();
    let mut open = true;
    // 跑起来之后不给叉。这一档关窗关不掉任何东西——请求已经在服务端跑了。
    let closable = !matches!(vm, Vm::Running { .. });
    let mut window = egui::Window::new(vm.kind().title())
        .id(egui::Id::new("plant-model-mutation"))
        .collapsible(false)
        .resizable(false)
        .default_width(460.0);
    if closable {
        window = window.open(&mut open);
    }
    window.show(ctx, |ui| {
        body(ui, t, d, vm, &mut cmds);
    });
    // 关窗等于不做。窗口右上角那个叉与「取消」必须是同一件事，不然人以为
    // 自己躲开了这个决定。
    if closable && !open {
        cmds.push(Cmd::ModelMutationConfirm {
            accepted: false,
            purge_meshes: false,
        });
    }
    cmds
}

fn body(ui: &mut egui::Ui, t: &Tokens, d: Density, vm: &mut Vm, cmds: &mut Vec<Cmd>) {
    match vm {
        Vm::Confirm {
            kind,
            label,
            dbnum,
            purge_meshes,
        } => {
            // 两格都是 Copy，先抄出来：`kind` 后面还要用来查表（`kind.note()` 等），
            // `dbnum` 也要在渲染那一格时再看一眼。
            let (kind, dbnum) = (*kind, *dbnum);
            let prompt = match (kind, dbnum) {
                (Kind::Cleanup, Some(dbnum)) => dbnum_cleanup_prompt(dbnum),
                _ => confirm_prompt(kind, label),
            };
            let note = if dbnum.is_some() {
                "不自动重算。同库有任务在跑或有生成在飞时服务端会拒绝，稍后再试。"
            } else {
                kind.note()
            };
            ui.label(RichText::new(prompt).color(t.text_primary));
            ui.add_space(d.px(6.0));
            ui.label(RichText::new(note).small().color(t.text_muted));
            // 「一并删除网格文件」那一格（2026-10-23）：两个清理档都有它——右键「清理模型」
            // 走子树、库行「清理模型（整库）」走整库，两条服务端接口都收这个参数。默认勾上，
            // 但要摆在按钮上面：网格是内容寻址的，别的范围复用的同一份也会跟着没，这一按的
            // 后果得在按之前看得见。整库那一档范围更大，撞上共享网格的机会也更大。
            if kind == Kind::Cleanup {
                ui.add_space(d.px(6.0));
                ui.checkbox(purge_meshes, "一并删除网格文件（.mesh）");
                ui.label(
                    RichText::new(
                        "网格文件是内容寻址的、可能被其他范围复用：勾上后它们引用的同一份也会一起\
                         删掉，那些范围要重新生成才会补回来。",
                    )
                    .small()
                    .color(t.text_muted),
                );
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .add(
                        widgets::button(t, d, kind.confirm_label())
                            .icon(kind.confirm_icon())
                            .primary(),
                    )
                    .clicked()
                {
                    cmds.push(Cmd::ModelMutationConfirm {
                        accepted: true,
                        purge_meshes: *purge_meshes,
                    });
                }
                if ui.add(widgets::button(t, d, "取消")).clicked() {
                    cmds.push(Cmd::ModelMutationConfirm {
                        accepted: false,
                        purge_meshes: false,
                    });
                }
            });
        }
        Vm::WaitingResident {
            kind,
            label,
            phase,
            files,
            meshes,
            dbnums,
        } => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(d.px(14.0)));
                ui.label(RichText::new(format!(
                    "{label}：正在重建房间参照模型与房间关系…"
                )));
            });
            ui.label(
                RichText::new(format!("阶段：{}", resident_phase_label(phase)))
                    .small()
                    .color(t.text_muted),
            );
            if let Some((done, total)) = files {
                if *total > 0 {
                    ui.add(
                        egui::ProgressBar::new(*done as f32 / *total as f32)
                            .text(format!("设计库文件 {done}/{total}")),
                    );
                }
            }
            if let Some((done, total, failed)) = meshes {
                if *total > 0 {
                    let text = if *failed > 0 {
                        format!("面板网格 {done}/{total}（失败 {failed}）")
                    } else {
                        format!("面板网格 {done}/{total}")
                    };
                    ui.add(
                        egui::ProgressBar::new(*done as f32 / *total as f32).text(text),
                    );
                }
            }
            if let Some((done, total)) = dbnums {
                if *total > 0 {
                    ui.add(
                        egui::ProgressBar::new(*done as f32 / *total as f32)
                            .text(format!("房间边重算 {done}/{total} 库")),
                    );
                }
            }
            ui.label(
                RichText::new(match kind {
                    Kind::RoomRebuild => {
                        "已删除全部房间归属并从头重建；逐库重算跑完才收摊，关窗只摘这一屏、服务端照跑。"
                    }
                    _ => {
                        "房间参照模型只在服务端没有房间数据时全量初始化一次，之后按增量维护。"
                    }
                })
                .small()
                .color(t.text_muted),
            );
            ui.separator();
            ui.horizontal(|ui| {
                // 「后台继续」：重建那发已经发出、服务端收不回——关窗只摘进度屏。
                // 其他 kind（若有）仍是「取消等待」：它们的请求还没发。
                let leave = match kind {
                    Kind::RoomRebuild => "后台继续",
                    _ => "取消等待",
                };
                if ui.add(widgets::button(t, d, leave)).clicked() {
                    cmds.push(Cmd::ModelMutationConfirm {
                    accepted: false,
                    purge_meshes: false,
                });
                }
            });
        }
        Vm::Running { kind, label } => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(d.px(14.0)));
                ui.label(RichText::new(kind.running_line(label)));
            });
            ui.label(RichText::new(kind.running_note()).small().color(t.text_muted));
        }
        Vm::Failed {
            kind,
            label,
            reason,
        } => {
            ui.colored_label(t.danger, format!("{label} {}：{reason}", kind.failed_verb()));
            ui.separator();
            ui.horizontal(|ui| {
                if ui.add(widgets::button(t, d, "关闭")).clicked() {
                    cmds.push(Cmd::ModelMutationConfirm {
                    accepted: false,
                    purge_meshes: false,
                });
                }
            });
        }
    }
}

/// `progress.phase` 的人话：阶段字面是服务端的机器名，界面上翻成这一屏在干什么。
fn resident_phase_label(phase: &str) -> &str {
    match phase {
        "spatial-tree" => "装载空间树",
        "source-snapshot" => "按 MDB 设计库快照建房间拓扑",
        "mesh-warm" => "预热面板网格",
        "edge-recompute" => "逐库重算房间关系",
        other => other,
    }
}

/// 确认框的语气。`Warn` 给会改模型库的那几档（等待档也算——它背后是同一趟），
/// 失败是 `Error`。
pub fn tone(vm: &Vm) -> Status {
    match vm {
        Vm::Confirm { .. } | Vm::WaitingResident { .. } | Vm::Running { .. } => Status::Warn,
        Vm::Failed { .. } => Status::Error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 重新生成的确认框必须把「已有产物会被重算替换」说出来——这是这一按唯一收不回来的后果，
    /// 而且不带数字：数字要靠清点，清点那条路随 ADR-0030 退役。
    #[test]
    fn the_regenerate_prompt_names_the_replacement_and_carries_no_count() {
        let prompt = confirm_prompt(Kind::Regenerate, "/ZONE-A");
        assert_eq!(
            prompt,
            "对 /ZONE-A 重新生成模型？范围内已有的产物会被重算替换。"
        );
        assert!(
            !prompt.contains("删除"),
            "不再说删除：删旧是服务端的事 {prompt}"
        );
        assert!(
            !prompt.chars().any(|c| c.is_ascii_digit()),
            "正文不摆清点数字：{prompt}"
        );
        assert!(
            confirm_prompt(Kind::Regenerate, "/ZONE-A 等 3 项").starts_with("对 /ZONE-A 等 3 项 ")
        );
    }

    /// 清理的确认框必须说出「删除」与「卸载」（这一按收不回来的两个后果）、指路「重新生成」，
    /// 小字说清不删设计元素；同样不带数字。
    #[test]
    fn the_cleanup_prompt_names_deletion_and_points_back_to_regeneration() {
        let prompt = confirm_prompt(Kind::Cleanup, "/ZONE-A");
        assert!(prompt.starts_with("对 /ZONE-A 清理模型？"), "{prompt}");
        for required in ["删除", "卸载", "重新生成模型"] {
            assert!(prompt.contains(required), "{required}: {prompt}");
        }
        assert!(
            !prompt.chars().any(|c| c.is_ascii_digit()),
            "正文不摆清点数字：{prompt}"
        );
        let note = Kind::Cleanup.note();
        assert!(note.contains("不删设计元素"), "{note}");
        assert!(
            !note.contains("不删共享的单位网格"),
            "网格那一格已经交给人勾了，note 里那句旧承诺不再成立：{note}"
        );
        assert_eq!(Kind::Cleanup.title(), "清理模型");
        assert_eq!(Kind::Regenerate.title(), "重新生成模型");
    }

    /// 「一并删除网格文件」那一格（2026-10-23）：两个清理档都画它——右键「清理模型」走
    /// `DELETE /model/subtree`、库行「清理模型（整库）」走 `DELETE /dbnums/{dbnum}/model`，
    /// 两条接口都收这个参数；重新生成（`force = true`）不画，它先删后算、网格删了要整批重写。
    /// 勾选状态随确认回执一起交出去（`accepted` 那一发带上它，其余出口一律 `false`）。
    /// 默认值由宿主开框时给（`main.rs` 的 `begin_mutation` / `begin_dbnum_model_cleanup`，
    /// 都默认勾上），这一层只管画与交。
    #[test]
    fn the_mesh_purge_checkbox_belongs_to_the_cleanup_dialog_only() {
        let source = include_str!("model_mutation.rs");
        let body = source.split_once("mod tests {").expect("测试模块").0;
        let confirm = body
            .split_once("Vm::Confirm {")
            .expect("确认那一臂")
            .1
            .split_once("Vm::WaitingResident {")
            .expect("下一臂")
            .0;
        assert!(confirm.contains("ui.checkbox(purge_meshes,"), "{confirm}");
        assert!(
            confirm.contains("if kind == Kind::Cleanup {"),
            "两个清理档都画它，整库那一档不再排除：{confirm}"
        );
        assert!(
            !confirm.contains("dbnum.is_none()"),
            "整库清理那一档今天也收这个参数，不再按 dbnum 分档：{confirm}"
        );
        assert!(
            confirm.contains("可能被其他范围复用"),
            "共享网格的后果要写在格边上：{confirm}"
        );
        assert!(
            confirm.contains("purge_meshes: *purge_meshes,"),
            "确认那一发把勾选状态原样带走：{confirm}"
        );
        // 其余出口（取消 / 关窗 / 取消等待 / 关闭失败态）一律 false：没有框在问这件事。
        let elsewhere = body
            .split_once("Vm::WaitingResident {")
            .expect("下一臂")
            .1;
        assert!(
            !elsewhere.contains("purge_meshes: true"),
            "别的出口不许凭空打开这一格：{elsewhere}"
        );
        assert_eq!(
            body.matches("purge_meshes: false,").count(),
            4,
            "取消 / 关窗 / 取消等待 / 关闭：{body}"
        );
    }

    /// 没有「停在这里」也没有「取消」能停掉在途那一发：跑起来之后窗上一个按钮都没有，
    /// 也没有叉——服务端连超时都不杀后台任务，承诺不了的事不摆按钮。两种 kind 共用这一臂。
    #[test]
    fn a_running_dialog_offers_no_way_out() {
        let source = include_str!("model_mutation.rs");
        let running = source
            .split_once("Vm::Running { kind, label } => {")
            .expect("Running 那一臂")
            .1
            .split_once("Vm::Failed {")
            .expect("下一臂")
            .0;
        assert!(!running.contains("widgets::button("), "{running}");
        assert!(!running.contains("Cmd::"), "{running}");
        let body = source.split_once("mod tests {").expect("测试模块").0;
        for retired in [
            "STOP_LABEL",
            "RegenerateStop",
            "Counting",
            "Plan {",
            "Progress {",
        ] {
            assert!(!body.contains(retired), "旧流水的形状还在：{retired}");
        }
        // 两件事一个框：Running 臂只有一条，按 kind 查表，不许为清理再复制一臂。
        assert_eq!(
            body.matches("Vm::Running { kind, label } => {").count(),
            1,
            "{body}"
        );
    }

    /// 「等房间参照模型」那一档给得出一个离开口：「重新生成房间」的请求已发出、
    /// 服务端收不回，按钮是「后台继续」（只摘进度屏）；其他用途还没发，按钮是
    /// 「取消等待」。发出之后（`Running`）照旧一个按钮都没有。两档纪律相反，不许共用一臂。
    #[test]
    fn the_resident_wait_is_cancellable_but_the_running_shot_is_not() {
        let source = include_str!("model_mutation.rs");
        let body = source.split_once("mod tests {").expect("测试模块").0;
        let waiting = body
            .split_once("Vm::WaitingResident {")
            .expect("WaitingResident 那一臂")
            .1
            .split_once("Vm::Running { kind, label } => {")
            .expect("下一臂")
            .0;
        assert!(waiting.contains("取消等待"), "{waiting}");
        assert!(waiting.contains("后台继续"), "{waiting}");
        assert!(waiting.contains("ModelMutationConfirm"), "{waiting}");
        // 叉只挡 `Running`：等待档关窗 = 取消等待，与按钮同一出口。
        let closable = body
            .split_once("let closable = ")
            .expect("closable 判定")
            .1
            .split(';')
            .next()
            .expect("closable 表达式");
        assert!(
            closable.contains("Vm::Running { .. }") && !closable.contains("WaitingResident"),
            "{closable}"
        );
    }

    /// 两档会改模型库的都是 `Warn`，失败是 `Error`；两种 kind 同一口径。
    #[test]
    fn destructive_states_carry_the_warning_tone() {
        let label = || "/ZONE-A".to_owned();
        for kind in [Kind::Regenerate, Kind::Cleanup, Kind::RoomRebuild] {
            assert_eq!(
                tone(&Vm::Confirm {
                    kind,
                    label: label(),
                    dbnum: None,
                    purge_meshes: true,
                }),
                Status::Warn
            );
            assert_eq!(
                tone(&Vm::Running {
                    kind,
                    label: label()
                }),
                Status::Warn
            );
            assert_eq!(
                tone(&Vm::Failed {
                    kind,
                    label: label(),
                    reason: "x".into()
                }),
                Status::Error
            );
        }
        let running = Vm::Running {
            kind: Kind::Cleanup,
            label: label(),
        };
        assert_eq!(running.label(), "/ZONE-A");
        assert_eq!(running.kind(), Kind::Cleanup);
    }
}
