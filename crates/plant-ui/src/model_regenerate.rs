//! 「重新生成模型」的确认框（ADR-0030 D2）。
//!
//! 这一步不发任何请求，它存在的唯一理由和模型更新的确认步一样：**按下去的是
//! 一个收不回来的按钮**——范围内已有的产物会被服务端重算替换。确认框不带数字：
//! 客户端不清点、不解根、不删产物，解根 / 删旧 / 重算全在服务端那一发
//! `POST /model/generate { force: true }` 里；此前「清点 → 数字确认 → 整片删 → 逐根调 →
//! 停在这里」那条流水随 ADR-0030 退役。

use egui::RichText;

use crate::Cmd;
use crate::style::tokens::{Density, Status, Tokens};
use crate::style::widgets;

/// 确认框此刻在说什么。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Vm {
    /// 等人按。还什么都没发。
    Confirm { label: String },
    /// 那一发 generate 在途。**这一档没有回头路**，所以窗口右上角那个叉也不给：服务端是
    /// `await_background_without_cancelling`，连 120 s 超时都不杀后台任务，关窗关不掉任何东西。
    Running { label: String },
    /// 生成失败。服务端那边没做成（或做了一半由它自己收拾），这里只有关闭。
    Failed { label: String, reason: String },
}

impl Vm {
    /// 窗上写的那个范围（右键落点的显示名，多选时形如 `/VESSEL-01 等 3 项`）。
    pub fn label(&self) -> &str {
        match self {
            Self::Confirm { label } | Self::Running { label } | Self::Failed { label, .. } => label,
        }
    }
}

/// 确认框正文。不带数字：数字要靠清点，而清点那条路已经退役；人要判断的只有
/// 「这个范围要不要重算」。
pub fn confirm_prompt(label: &str) -> String {
    format!("对 {label} 重新生成模型？范围内已有的产物会被重算替换。")
}

pub fn show(ctx: &egui::Context, t: &Tokens, d: Density, vm: Option<&Vm>) -> Vec<Cmd> {
    let Some(vm) = vm else {
        return Vec::new();
    };

    let mut cmds = Vec::new();
    let mut open = true;
    // 跑起来之后不给叉。这一档关窗关不掉任何东西——请求已经在服务端跑了。
    let closable = !matches!(vm, Vm::Running { .. });
    let mut window = egui::Window::new("重新生成模型")
        .id(egui::Id::new("plant-model-regenerate"))
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
        cmds.push(Cmd::RegenerateConfirm { accepted: false });
    }
    cmds
}

fn body(ui: &mut egui::Ui, t: &Tokens, d: Density, vm: &Vm, cmds: &mut Vec<Cmd>) {
    match vm {
        Vm::Confirm { label } => {
            ui.label(RichText::new(confirm_prompt(label)).color(t.text_primary));
            ui.add_space(d.px(6.0));
            ui.label(
                RichText::new(
                    "服务端自己解根、重算；跑完会把这个范围的模型显示出来。大范围可能超过服务端 \
                     120 秒预算，那时服务端后台继续，完成后再点眼睛查看。",
                )
                .small()
                .color(t.text_muted),
            );
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .add(
                        widgets::button(t, d, "重新生成")
                            .icon(egui_phosphor::regular::ARROWS_CLOCKWISE)
                            .primary(),
                    )
                    .clicked()
                {
                    cmds.push(Cmd::RegenerateConfirm { accepted: true });
                }
                if ui.add(widgets::button(t, d, "取消")).clicked() {
                    cmds.push(Cmd::RegenerateConfirm { accepted: false });
                }
            });
        }
        Vm::Running { label } => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(d.px(14.0)));
                ui.label(RichText::new(format!("正在生成 {label} 的模型…")));
            });
            ui.label(
                RichText::new("服务端正在重算，停不下来；跑完自动显示。")
                    .small()
                    .color(t.text_muted),
            );
        }
        Vm::Failed { label, reason } => {
            ui.colored_label(t.danger, format!("{label} 生成失败：{reason}"));
            ui.separator();
            ui.horizontal(|ui| {
                if ui.add(widgets::button(t, d, "关闭")).clicked() {
                    cmds.push(Cmd::RegenerateConfirm { accepted: false });
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

    /// 确认框必须把「已有产物会被重算替换」说出来——这是这一按唯一收不回来的后果，
    /// 而且不带数字：数字要靠清点，清点那条路随 ADR-0030 退役。
    #[test]
    fn the_confirm_prompt_names_the_replacement_and_carries_no_count() {
        let prompt = confirm_prompt("/ZONE-A");
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
        assert!(confirm_prompt("/ZONE-A 等 3 项").starts_with("对 /ZONE-A 等 3 项 "));
    }

    /// 没有「停在这里」也没有「取消」能停掉在途那一发：跑起来之后窗上一个按钮都没有，
    /// 也没有叉——服务端连超时都不杀后台任务，承诺不了的事不摆按钮。
    #[test]
    fn a_running_dialog_offers_no_way_out() {
        let source = include_str!("model_regenerate.rs");
        let running = source
            .split_once("Vm::Running { label } => {")
            .expect("Running 那一臂")
            .1
            .split_once("Vm::Failed { label, reason } => {")
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
    }

    /// 两档会改模型库的都是 `Warn`，失败是 `Error`。
    #[test]
    fn destructive_states_carry_the_warning_tone() {
        let label = || "/ZONE-A".to_owned();
        assert_eq!(tone(&Vm::Confirm { label: label() }), Status::Warn);
        assert_eq!(tone(&Vm::Running { label: label() }), Status::Warn);
        assert_eq!(
            tone(&Vm::Failed {
                label: label(),
                reason: "x".into()
            }),
            Status::Error
        );
        assert_eq!(Vm::Running { label: label() }.label(), "/ZONE-A");
    }
}
