//! M0-6 组件画廊（`--gallery` 启动参数进入）。
//! 保留它：在真数据面板出问题时，这里是零风险验证组件观感的地方。

use eframe::egui;
use plant_ui::style::theme_tokens;
use plant_ui::style::tokens::{Density, Status};
use plant_ui::style::widgets::{self, RowIcon, RowTag, TreeRow};

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

            ui.add_space(12.0);
            let rows: [(usize, &str, Option<bool>, bool); 3] = [
                (0, "SITE /-RX-CSV", Some(true), false),
                (1, "ZONE /-RX-CSV-S4009", Some(true), false),
                (
                    2,
                    "STRU /-RX-CSV-S4009-V1 一段很长的中文名称用来验证截断",
                    Some(false),
                    true,
                ),
            ];
            for (depth, label, expandable, selected) in rows {
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
                        expandable,
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
