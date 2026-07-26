//! plant-ui-app：独立可执行外壳（eframe + plant-ui + plant-ui-data）。
//! M0 阶段内容是组件画廊；M1 换成主工作台 S1。

mod fonts;

use eframe::egui;
use plant_ui::style::theme_tokens::{self, set_weight_families_ready};
use plant_ui::style::tokens::{Density, Status, Tokens};
use plant_ui::style::widgets::{self, RowIcon, RowTag, TreeRow};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 1000.0])
            .with_title("plant-ui"),
        ..Default::default()
    };
    eframe::run_native(
        "plant-ui",
        options,
        Box::new(|cc| {
            let defs = fonts::definitions();
            let weights_ready = defs.font_data.contains_key(fonts::MEDIUM);
            cc.egui_ctx.set_fonts(defs);
            set_weight_families_ready(weights_ready);
            theme_tokens::apply(&cc.egui_ctx, &Tokens::dark(), Density::Standard);
            Ok(Box::new(App::default()))
        }),
    )
}

struct App {
    vm: plant_ui::vm::WorkbenchVm,
    chip_on: bool,
    toggle_on: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            vm: plant_ui::vm::WorkbenchVm {
                project: "AvevaMarineSample".into(),
                element_count: 0,
                selected: None,
            },
            chip_on: true,
            toggle_on: true,
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let t = theme_tokens::current();
        let d = Density::Standard;
        egui::CentralPanel::default().show(ui, |ui| {
            widgets::section_header(
                ui,
                &t,
                d,
                "plant-ui · M0 组件画廊",
                "egui 0.35 · 组件层迁自 ui/redesign",
            );
            ui.add_space(12.0);

            ui.horizontal(|ui| {
                let _ = ui.add(widgets::button(&t, d, "主要动作").primary());
                let _ = ui.add(widgets::button(&t, d, "次要动作"));
                if ui.add(widgets::chip(&t, d, "ERROR 12", self.chip_on)).clicked() {
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
                (2, "STRU /-RX-CSV-S4009-V1 一段很长的中文名称用来验证截断", Some(false), true),
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

            ui.add_space(12.0);
            ui.label(format!(
                "project: {}  elements: {}",
                self.vm.project, self.vm.element_count
            ));
        });
    }
}