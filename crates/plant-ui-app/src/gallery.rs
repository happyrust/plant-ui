//! M0-6 组件画廊（`--gallery` 启动参数进入）。
//! 保留它：在真数据面板出问题时，这里是零风险验证组件观感的地方。

use eframe::egui;
use plant_ui::style::theme_tokens;
use plant_ui::style::tokens::{Density, Status};
use plant_ui::style::widgets::{self, Eye, RowIcon, RowTag, TreeRow};

pub struct GalleryApp {
    chip_on: bool,
    toggle_on: bool,
}

impl Default for GalleryApp {
    fn default() -> Self {
        Self {
            chip_on: true,
            toggle_on: true,
        }
    }
}

impl eframe::App for GalleryApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let t = theme_tokens::current();
        let d = Density::Standard;
        egui::CentralPanel::default().show(ui, |ui| {
            widgets::section_header(
                ui,
                &t,
                d,
                "plant-ui · 组件画廊",
                "egui 0.35 · 组件层迁自 ui/redesign",
            );
            ui.add_space(12.0);

            ui.horizontal(|ui| {
                let _ = ui.add(widgets::button(&t, d, "主要动作").primary());
                let _ = ui.add(widgets::button(&t, d, "次要动作"));
                if ui
                    .add(widgets::chip(&t, d, "ERROR 12", self.chip_on))
                    .clicked()
                {
                    self.chip_on = !self.chip_on;
                }
                let _ = ui.add(widgets::status_tag(&t, d, "已连接", Status::Success));
                let _ = ui.add(widgets::status_tag(&t, d, "3 个错误", Status::Error));
                let _ = ui.add(widgets::toggle(&t, d, &mut self.toggle_on));
            });

            // 工具按钮的三态并排：常态 / 激活 / 禁用。视口工具栏没选中元素时靠
            // 禁用态说话，画廊里不摆一份就没人看得出它和常态到底有没有分别。
            //
            // 后两枚是视口工具栏上「隐藏全部 / 显示全部」那一对，摆在一起才看得出
            // 闭眼与复数眼这一对读不读得成「全部」——单看任何一枚都判断不了。
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let _ = ui.add(widgets::tool_btn(&t, d, egui_phosphor::regular::EYE, false));
                let _ = ui.add(widgets::tool_btn(
                    &t,
                    d,
                    egui_phosphor::regular::EYE_SLASH,
                    true,
                ));
                let _ = ui.add_enabled(
                    false,
                    widgets::tool_btn(&t, d, egui_phosphor::regular::CROSSHAIR_SIMPLE, false),
                );
                ui.add_space(12.0);
                let _ = ui.add(widgets::tool_btn(
                    &t,
                    d,
                    egui_phosphor::regular::EYE_CLOSED,
                    false,
                ));
                let _ = ui.add(widgets::tool_btn(
                    &t,
                    d,
                    egui_phosphor::regular::EYES,
                    false,
                ));
            });

            ui.add_space(12.0);
            // 后两行都在选择集里，只有末行是主选中：多选下的主 / 从两种描边
            // 就是靠这两行并排看出来的。
            //
            // 五行的眼睛各占一档：首行是「未选中 + 已隐藏」，验整行弱化；末行是
            // 「选中 + 已隐藏」，验选中优先于弱化；第三行未加载，要把鼠标移上去
            // 才画得出来——但那一格的宽度**任何时候都占着**，`24381/100060` 的
            // 位置不该随悬停移动。第四行是半可见，半闭的眼要与斜杠眼分得开。
            let rows: [(usize, &str, Option<bool>, bool, bool, Eye); 5] = [
                (
                    0,
                    "SITE /-RX-CSV-OLD",
                    Some(false),
                    false,
                    false,
                    Eye::Hidden,
                ),
                (0, "SITE /-RX-CSV", Some(true), false, false, Eye::Shown),
                (
                    1,
                    "ZONE /-RX-CSV-S4009",
                    Some(true),
                    true,
                    false,
                    Eye::Unloaded,
                ),
                (
                    1,
                    "ZONE /-RX-CSV-S4010",
                    Some(false),
                    false,
                    false,
                    Eye::Partial,
                ),
                (
                    2,
                    "STRU /-RX-CSV-S4009-V1 一段很长的中文名称用来验证截断",
                    Some(false),
                    true,
                    true,
                    Eye::Hidden,
                ),
            ];
            for (depth, label, expandable, selected, primary, eye) in rows {
                let _ = widgets::tree_row_ui(
                    ui,
                    &t,
                    d,
                    TreeRow {
                        depth,
                        icon: RowIcon::Glyph(egui_phosphor::regular::CUBE),
                        label,
                        meta: "24381/100060",
                        selected,
                        primary,
                        expandable,
                        eye: Some(eye),
                        tone: Status::Neutral,
                        tags: &[RowTag {
                            icon: None,
                            text: "DESI",
                            tone: Status::Info,
                        }],
                    },
                );
            }
        });
    }
}
