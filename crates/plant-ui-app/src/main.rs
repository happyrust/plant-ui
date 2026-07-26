//! plant-ui-app：独立可执行外壳（eframe + plant-ui + plant-ui-data）。
//! 默认进入主工作台 S1；`--gallery` 进入 M0 组件画廊。

mod fonts;
mod gallery;

use eframe::egui;
use plant_ui::style::theme_tokens::{self, set_weight_families_ready};
use plant_ui::style::tokens::{Density, Tokens};
use plant_ui::vm::WorkbenchVm;
use plant_ui::workbench::{self, WorkbenchState};

fn main() -> eframe::Result {
    let gallery = std::env::args().any(|a| a == "--gallery");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 1000.0])
            .with_title("plant-ui"),
        ..Default::default()
    };
    eframe::run_native(
        "plant-ui",
        options,
        Box::new(move |cc| {
            let defs = fonts::definitions();
            let weights_ready = defs.font_data.contains_key(fonts::MEDIUM);
            cc.egui_ctx.set_fonts(defs);
            set_weight_families_ready(weights_ready);
            theme_tokens::apply(&cc.egui_ctx, &Tokens::dark(), Density::Standard);
            if gallery {
                Ok(Box::new(gallery::GalleryApp::default()))
            } else {
                Ok(Box::new(App::default()))
            }
        }),
    )
}

struct App {
    vm: WorkbenchVm,
    state: WorkbenchState,
}

impl Default for App {
    fn default() -> Self {
        Self {
            // M1-1 外壳阶段的真实状态：还没连库，所以数据源「未连接」、元素 0。
            // M1-2 接 plant-ui-data 后这些字段来自真实查询。
            vm: WorkbenchVm {
                project: "AvevaMarineSample".into(),
                db: "ns 1516 / db 7997".into(),
                user: std::env::var("USERNAME").unwrap_or_else(|_| "user".into()),
                data_source_ok: false,
                element_count: 0,
                selected: None,
            },
            state: WorkbenchState::default(),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let t = theme_tokens::current();
        let d = Density::Standard;
        let cmds = workbench::show(ui, &t, d, &self.vm, &mut self.state);
        // M1-6 联动前，命令先落日志，验证 Vm 进 / Cmd 出的管道是通的。
        for cmd in cmds {
            eprintln!("[cmd] {cmd:?}");
        }
    }
}
