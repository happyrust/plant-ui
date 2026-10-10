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

/// 这一框在问哪一件事。两件都改模型库，差别只在文案与按下去之后服务端做什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 右键「重新生成模型」：服务端先删后生成，跑完显示。
    Regenerate,
    /// 右键「清理模型」：服务端只删产物、不重算；跑完把三维里对应的模型卸掉。
    Cleanup,
}

impl Kind {
    /// 窗标题，也是右键菜单上那一项的名字。
    pub fn title(self) -> &'static str {
        match self {
            Self::Regenerate => "重新生成模型",
            Self::Cleanup => "清理模型",
        }
    }

    /// 确认按钮上的动词。
    fn confirm_label(self) -> &'static str {
        match self {
            Self::Regenerate => "重新生成",
            Self::Cleanup => "清理",
        }
    }

    fn confirm_icon(self) -> &'static str {
        match self {
            Self::Regenerate => egui_phosphor::regular::ARROWS_CLOCKWISE,
            Self::Cleanup => egui_phosphor::regular::BROOM,
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
                "只删已生成的产物，不删设计元素、不删共享的单位网格；服务端按子树删，范围内有根 \
                 正在生成会被拒绝（稍后再试）。跑完三维里对应的模型会卸载。"
            }
        }
    }

    fn running_line(self, label: &str) -> String {
        match self {
            Self::Regenerate => format!("正在生成 {label} 的模型…"),
            Self::Cleanup => format!("正在清理 {label} 的模型产物…"),
        }
    }

    fn running_note(self) -> &'static str {
        match self {
            Self::Regenerate => "服务端正在重算，停不下来；跑完自动显示。",
            Self::Cleanup => "服务端正在删除，停不下来；跑完自动卸载对应的模型。",
        }
    }

    fn failed_verb(self) -> &'static str {
        match self {
            Self::Regenerate => "生成失败",
            Self::Cleanup => "清理失败",
        }
    }
}

/// 确认框此刻在说什么。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Vm {
    /// 等人按。还什么都没发。
    Confirm { kind: Kind, label: String },
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
            Self::Confirm { label, .. } | Self::Running { label, .. } | Self::Failed { label, .. } => {
                label
            }
        }
    }

    pub fn kind(&self) -> Kind {
        match self {
            Self::Confirm { kind, .. } | Self::Running { kind, .. } | Self::Failed { kind, .. } => {
                *kind
            }
        }
    }
}

/// 确认框正文。不带数字：数字要靠清点，而清点那条路已经退役；人要判断的只有
/// 「这个范围要不要重算 / 清掉」。
pub fn confirm_prompt(kind: Kind, label: &str) -> String {
    match kind {
        Kind::Regenerate => format!("对 {label} 重新生成模型？范围内已有的产物会被重算替换。"),
        Kind::Cleanup => format!(
            "对 {label} 清理模型？范围内已生成的产物会被删除，三维里对应的模型会卸载；需要时再右键「重新生成模型」。"
        ),
    }
}

pub fn show(ctx: &egui::Context, t: &Tokens, d: Density, vm: Option<&Vm>) -> Vec<Cmd> {
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
        cmds.push(Cmd::ModelMutationConfirm { accepted: false });
    }
    cmds
}

fn body(ui: &mut egui::Ui, t: &Tokens, d: Density, vm: &Vm, cmds: &mut Vec<Cmd>) {
    match vm {
        Vm::Confirm { kind, label } => {
            ui.label(RichText::new(confirm_prompt(*kind, label)).color(t.text_primary));
            ui.add_space(d.px(6.0));
            ui.label(RichText::new(kind.note()).small().color(t.text_muted));
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
                    cmds.push(Cmd::ModelMutationConfirm { accepted: true });
                }
                if ui.add(widgets::button(t, d, "取消")).clicked() {
                    cmds.push(Cmd::ModelMutationConfirm { accepted: false });
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
                    cmds.push(Cmd::ModelMutationConfirm { accepted: false });
                }
            });
        }
    }
}

/// 确认框的语气。`Warn` 给会改模型库的那两档，失败是 `Error`。
pub fn tone(vm: &Vm) -> Status {
    match vm {
        Vm::Confirm { .. } | Vm::Running { .. } => Status::Warn,
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
        assert!(note.contains("单位网格"), "d-8：清理不删共享单位网格 {note}");
        assert_eq!(Kind::Cleanup.title(), "清理模型");
        assert_eq!(Kind::Regenerate.title(), "重新生成模型");
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

    /// 两档会改模型库的都是 `Warn`，失败是 `Error`；两种 kind 同一口径。
    #[test]
    fn destructive_states_carry_the_warning_tone() {
        let label = || "/ZONE-A".to_owned();
        for kind in [Kind::Regenerate, Kind::Cleanup] {
            assert_eq!(
                tone(&Vm::Confirm {
                    kind,
                    label: label()
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
