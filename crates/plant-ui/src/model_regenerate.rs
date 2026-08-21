//! 「重新生成模型」的确认框。
//!
//! 这一步不发任何请求，它存在的唯一理由和模型更新的确认步一样：**按下去的是
//! 一个收不回来的按钮**。区别在于它有数字可摆——deep query 本来就必须跑在删除
//! 之前，跑完手上正好有「多少个已生成元素」和「归成多少个生成单元」。右键一根
//! BRAN 时它显示 1 和 1，弹一下不痛不痒；右键错一个 SITE 时它当场把人拦下来。

use egui::RichText;

use crate::Cmd;
use crate::style::tokens::{Density, Status, Tokens};
use crate::style::widgets;

/// 「停在这里」而不是「取消」。
///
/// 已经发出去的那一个 ensure 停不了——服务端是
/// `await_background_without_cancelling`，连 120 秒超时都不杀后台任务。这一按
/// 的真实含义只有「不再派发下一个」，文案不许说得比这更多。
pub const STOP_LABEL: &str = "停在这里";

/// 确认框此刻在说什么。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Vm {
    /// deep query 在途。数字还没出来，这时候不许摆按钮——摆了就得先摆一个假的 0。
    Counting { label: String },
    /// 数字出来了，等人按。
    Ready(Plan),
    /// 清点失败。删除一步都还没走，所以这里只有关闭。
    Failed { label: String, reason: String },
}

/// 清点结果：这一按会付出的代价。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// 右键落点的显示名（多选时形如 `/VESSEL-01 等 3 项`）。
    pub label: String,
    /// `inst_relate` + `tubi_relate` 上属于这个范围的已生成元素数。
    pub elements: usize,
    /// 归并出来的生成单元数。是**上限**：嵌套交付单元与共根元素都还留在里面，
    /// 真正的去重由服务端的 `AlreadyAvailable` 免费完成。
    pub roots: usize,
}

/// 确认框正文。
///
/// 两个数字都来自真实查询，`roots` 标明是上限。第二句是本特性接受「整片删一次」
/// 之后唯一的补偿：中断之后没重做完的那些**找不回来**——deep query 认的是
/// `inst_relate` 行，删完就查不到了，重新右键同一个容器也再看不见它们。
pub fn confirm_prompt(plan: &Plan) -> String {
    format!(
        "将删除 {} 范围内 {} 个已生成元素，归成最多 {} 个生成单元重做。\n\
         中途中断的话，没重做完的那些找不回来。",
        plan.label, plan.elements, plan.roots
    )
}

pub fn show(ctx: &egui::Context, t: &Tokens, d: Density, vm: Option<&Vm>) -> Vec<Cmd> {
    let Some(vm) = vm else {
        return Vec::new();
    };

    let mut cmds = Vec::new();
    let mut open = true;
    egui::Window::new("重新生成模型")
        .id(egui::Id::new("plant-model-regenerate"))
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .default_width(460.0)
        .show(ctx, |ui| {
            body(ui, t, d, vm, &mut cmds);
        });
    // 关窗等于不做。窗口右上角那个叉与「取消」必须是同一件事，不然人以为
    // 自己躲开了这个决定，而那一趟已经开跑了。
    if !open {
        cmds.push(Cmd::RegenerateConfirm { accepted: false });
    }
    cmds
}

fn body(ui: &mut egui::Ui, t: &Tokens, d: Density, vm: &Vm, cmds: &mut Vec<Cmd>) {
    match vm {
        Vm::Counting { label } => {
            ui.label(RichText::new(format!(
                "正在清点 {label} 范围内已生成的模型…"
            )));
            ui.label(
                RichText::new("这一步只读，还没有删掉任何东西。")
                    .small()
                    .color(t.text_muted),
            );
            ui.separator();
            ui.horizontal(|ui| {
                if ui.add(widgets::button(t, d, "取消")).clicked() {
                    cmds.push(Cmd::RegenerateConfirm { accepted: false });
                }
            });
        }
        Vm::Failed { label, reason } => {
            ui.colored_label(t.danger, format!("{label} 清点失败：{reason}"));
            ui.label(
                RichText::new("没有删掉任何东西。")
                    .small()
                    .color(t.text_muted),
            );
            ui.separator();
            ui.horizontal(|ui| {
                if ui.add(widgets::button(t, d, "关闭")).clicked() {
                    cmds.push(Cmd::RegenerateConfirm { accepted: false });
                }
            });
        }
        // 查完是空的：这个范围下没有生成过模型，删无可删、也没什么可重做。
        // 只给关闭——摆一个按下去什么都不会发生的主按钮才是骗人。
        Vm::Ready(plan) if plan.elements == 0 => {
            ui.label(format!("{} 范围内没有已经生成的模型。", plan.label));
            ui.label(
                RichText::new("「重新生成」只重做已经生产过的那些；从没生成过的元素请用显示模型。")
                    .small()
                    .color(t.text_muted),
            );
            ui.separator();
            ui.horizontal(|ui| {
                if ui.add(widgets::button(t, d, "关闭")).clicked() {
                    cmds.push(Cmd::RegenerateConfirm { accepted: false });
                }
            });
        }
        Vm::Ready(plan) => {
            ui.label(RichText::new(confirm_prompt(plan)).color(t.text_primary));
            ui.add_space(d.px(6.0));
            ui.colored_label(t.warn, "跑完会把这些模型全部显示出来，包括本来隐藏的。");
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .add(
                        widgets::button(t, d, "删除并重新生成")
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
    }
}

/// 确认框的语气。`Warn` 只留给真会删东西的那一档——空集与失败都没有代价，
/// 给它们挂黄色只会让人对这个颜色脱敏。
pub fn tone(vm: &Vm) -> Status {
    match vm {
        Vm::Counting { .. } => Status::Info,
        Vm::Failed { .. } => Status::Error,
        Vm::Ready(plan) if plan.elements == 0 => Status::Neutral,
        Vm::Ready(_) => Status::Warn,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 确认框必须把「中断会丢」说出来，和两个真实数字一起。
    ///
    /// 这是本特性接受「整片删一次」之后唯一的补偿：删完之后 `inst_relate` 上
    /// 没有它们了，deep query 再也找不到那批元素，服务端也没有记录。少了这句话，
    /// 人按下去时并不知道自己在拿什么冒险。`model_update` 那边有一条同型断言。
    #[test]
    fn the_confirm_prompt_says_an_interruption_loses_work() {
        let prompt = confirm_prompt(&Plan {
            label: "/ZONE-A".into(),
            elements: 12431,
            roots: 340,
        });
        assert!(prompt.contains("12431"), "{prompt}");
        assert!(prompt.contains("340"), "{prompt}");
        assert!(prompt.contains("找不回来"), "{prompt}");
    }

    /// 上限就得说成上限。归根故意不裁剪嵌套交付单元（裁剪要复制服务端「哪个
    /// 祖先更近」那套判定），所以真跑下来的单元数只会比这个数小。把它说成
    /// 确数，收尾那句「340 个单元：成功 12」看着就像丢了三百多个。
    #[test]
    fn the_unit_count_is_stated_as_a_ceiling() {
        let prompt = confirm_prompt(&Plan {
            label: "/BRAN-1".into(),
            elements: 42,
            roots: 1,
        });
        assert!(prompt.contains("最多 1 个生成单元"), "{prompt}");
    }

    /// 「停在这里」不许写成「取消」。已经发出去的那一个 ensure 停不了，
    /// 服务端连超时都不杀后台任务——按钮上写「取消」就是承诺了一件做不到的事。
    #[test]
    fn the_stop_button_never_promises_a_cancel() {
        assert!(!STOP_LABEL.contains("取消"), "{STOP_LABEL}");
        assert!(!STOP_LABEL.contains("撤销"), "{STOP_LABEL}");
    }

    /// 空集不是警告。这一档按下去什么都不会发生，给它挂上删除那一档的黄色，
    /// 下次真要删几千个元素时人已经不看颜色了。
    #[test]
    fn only_a_destructive_plan_carries_the_warning_tone() {
        let plan = |elements| Plan {
            label: "/ZONE-A".into(),
            elements,
            roots: 1,
        };
        assert_eq!(tone(&Vm::Ready(plan(1))), Status::Warn);
        assert_eq!(tone(&Vm::Ready(plan(0))), Status::Neutral);
        assert_eq!(
            tone(&Vm::Counting {
                label: "/ZONE-A".into()
            }),
            Status::Info
        );
    }
}
