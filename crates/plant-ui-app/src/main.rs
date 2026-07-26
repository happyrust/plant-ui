//! plant-ui-app：独立可执行外壳。eframe + plant-ui + plant-ui-data + 占位视口。

use eframe::egui;

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
            let mut fonts = egui::FontDefinitions::default();
            egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(App::default()))
        }),
    )
}

struct App {
    vm: plant_ui::vm::WorkbenchVm,
}

impl Default for App {
    fn default() -> Self {
        Self {
            vm: plant_ui::vm::WorkbenchVm {
                project: "AvevaMarineSample".into(),
                element_count: 0,
                selected: None,
            },
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let _cmds = plant_ui::workbench_placeholder(ui, &self.vm);
        });
    }
}