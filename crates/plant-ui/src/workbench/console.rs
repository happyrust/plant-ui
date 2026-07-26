//! 命令行视图（M1-4）：分级 + 计数芯片筛选 + 错误行处置入口 + 文本可选中复制。
//!
//! 旧壳这一屏有过一次两难：换成组件层的绘制型 `log_row` 就丢了文本可选中复制，
//! 留着 `TextEdit` 就丢了分级配色，最后选了后者。新项目里 `log_row_ui` 的时间 /
//! 分级 / 正文都是 selectable 的 `Label`，两样可以同时要——见组件层那段说明。
//!
//! 分级也不再靠关键词猜。旧壳的 `PrintConsoleLine` 只带一个字符串，词表照真实
//! 语料校准过仍会错判；这里发日志的就是 App 自己，级别在发的那一刻定死。

use egui::{Align, Layout, Margin, RichText, ScrollArea, Ui};
use egui_phosphor::regular as ph;

use crate::Cmd;
use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Tokens, space};
use crate::style::widgets::{self, LogRow};
use crate::vm::{LogLevel, LogLineVm, WorkbenchVm};

pub fn show(ui: &mut Ui, t: &Tokens, d: Density, vm: &WorkbenchVm, cmds: &mut Vec<Cmd>) {
    // 筛选是界面自己的状态而非数据，跟属性面板的分组折叠一样存在 egui 的临时数据里，
    // 这样 show 的签名仍是 &Vm 进、Vec<Cmd> 出。
    let filter_id = ui.id().with("console-filter");
    let mut filter = ui.data_mut(|s| *s.get_temp_mut_or::<Option<LogLevel>>(filter_id, None));

    egui::Frame::new().fill(t.bg_panel).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        // 工具条、hairline、列表之间不能有间距，否则边框会被推离工具条。
        ui.spacing_mut().item_spacing.y = 0.0;
        if toolbar(ui, t, d, vm, &mut filter, cmds) {
            ui.data_mut(|s| s.insert_temp(filter_id, filter));
        }
        super::chrome::hairline(ui, t);
        list(ui, t, d, vm, filter, cmds);
    });
}

/// 工具条：分级计数芯片（点一下筛选，再点一下取消）+ 清空。
fn toolbar(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    vm: &WorkbenchVm,
    filter: &mut Option<LogLevel>,
    cmds: &mut Vec<Cmd>,
) -> bool {
    let mut changed = false;
    egui::Frame::new()
        .fill(t.bg_header)
        .inner_margin(Margin::symmetric(space::S2 as i8, 0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(d.px(32.0));
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = space::S1;
                let counts = vm.console.counts;
                let all = format!("全部 {}", counts.total());
                if ui
                    .add(widgets::chip(t, d, &all, filter.is_none()))
                    .clicked()
                {
                    *filter = None;
                    changed = true;
                }
                for level in LogLevel::ALL {
                    let label = format!("{} {}", level.label(), counts.of(level));
                    let on = *filter == Some(level);
                    if ui.add(widgets::chip(t, d, &label, on)).clicked() {
                        *filter = (!on).then_some(level);
                        changed = true;
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(widgets::tool_btn(t, d, ph::TRASH, false))
                        .on_hover_text("清空命令行")
                        .clicked()
                    {
                        cmds.push(Cmd::ClearConsole);
                    }
                });
            });
        });
    changed
}

fn list(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    vm: &WorkbenchVm,
    filter: Option<LogLevel>,
    cmds: &mut Vec<Cmd>,
) {
    // show_rows 要按下标取行，筛选后必须先落成一张可索引的表。缓冲是限长的，
    // 这张表也就是有界的。
    let lines: Vec<&LogLineVm> = vm
        .console
        .lines
        .iter()
        .filter(|l| filter.is_none_or(|f| l.level == f))
        .collect();

    if lines.is_empty() {
        let text = if vm.console.lines.is_empty() {
            "暂无输出"
        } else {
            "当前分级没有输出"
        };
        note(ui, t, d, ph::TERMINAL_WINDOW, text);
        return;
    }

    // show_rows 是拿**外层** ui 的 item_spacing.y 去折算每行占位的，设在闭包里就晚了：
    // 行按 24 画、位子却按 24+6 留，滚动条会比内容长出一截。
    ui.spacing_mut().item_spacing.y = 0.0;
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show_rows(ui, d.px(24.0), lines.len(), |ui, range| {
            for line in &lines[range] {
                row(ui, t, d, line, cmds);
            }
        });
}

fn row(ui: &mut Ui, t: &Tokens, d: Density, line: &LogLineVm, cmds: &mut Vec<Cmd>) {
    let out = widgets::log_row_ui(
        ui,
        t,
        d,
        LogRow {
            time: &line.time,
            level: line.level.status(),
            level_text: line.level.label(),
            element: line.element.as_ref().map(|e| e.label.as_str()),
            message: &line.message,
        },
        |ui| {
            // 处置入口只给错误行：其余行没有可重来的操作，摆上去只是噪音。
            if line.level != LogLevel::Error {
                return;
            }
            if line.retryable && ui.add(widgets::chip(t, d, "重试", false)).clicked() {
                cmds.push(Cmd::RetryLog(line.id));
            }
            if ui.add(widgets::chip(t, d, "复制", false)).clicked() {
                ui.ctx().copy_text(full_text(line));
            }
        },
    );
    // 正文按行宽截断，完整错误链挂 hover；要抄走的话行尾有「复制」，
    // 或者直接在行上拖选（时间 / 分级 / 正文都可选中）。
    if let Some(detail) = &line.detail {
        let _ = out.message.on_hover_text(detail);
    }
    if let (Some(resp), Some(el)) = (out.element, &line.element) {
        if resp.clicked() {
            cmds.push(Cmd::LocateElement(el.refno));
        }
        resp.on_hover_text(format!("定位到 {}", el.label));
    }
}

fn full_text(line: &LogLineVm) -> String {
    let head = format!("{} {} {}", line.time, line.level.label(), line.message);
    match &line.detail {
        Some(detail) if detail != &line.message => format!("{head}\n{detail}"),
        _ => head,
    }
}

/// 命令行区的居中提示（无输出 / 筛选后为空）。四态统一视觉 M1-7 收口。
fn note(ui: &mut Ui, t: &Tokens, d: Density, icon: &str, text: &str) {
    egui::Frame::new().fill(t.bg_panel).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        ui.vertical_centered(|ui| {
            ui.add_space(d.px(28.0));
            ui.label(
                RichText::new(icon)
                    .font(egui::FontId::proportional(d.px(24.0)))
                    .color(t.text_muted),
            );
            ui.add_space(space::S2);
            ui.label(
                RichText::new(text)
                    .font(Font::meta(d))
                    .color(t.text_secondary),
            );
        });
    });
}
