//! 模型树的行 id 必须只由 refno 决定，不能跟着行序走。
//!
//! egui 0.35 起，调试构建默认开 `Style::debug.warn_if_rect_changes_id`：一帧结束时它拿
//! 本帧与上一帧的控件矩形对表，同一块矩形换了 id、旧 id 又从这一层消失了，就给那块矩形
//! 描一圈 2px 纯红外边（还发一条 `Widget rect … changed id between passes` 的 warn）。
//!
//! 行 id 若混进了行序，展开一个节点会让**下方每一行**都换号，于是整屏红框闪一下。
//! 这里不看屏幕，直接从一帧的图元里数那种红框：测试跑在调试构建下，与用户看到的是同一条路径。

use aios_core::RefU64;
use egui::epaint::ClippedShape;
use egui::{Context, RawInput, Rect, Shape, pos2, vec2};
use plant_ui::style::tokens::{Density, Tokens};
use plant_ui::vm::{RowVisibility, TreeRowVm, TreeVm, WorkbenchVm};

/// 一帧里 `warn_if_rect_changes_id` 画出的红框。
fn red_boxes(shapes: &[ClippedShape]) -> Vec<String> {
    fn walk(shape: &Shape, out: &mut Vec<String>) {
        match shape {
            // 特征取死：2px、纯红、外描边、无圆角、不填充。别的红色描边不长这样。
            Shape::Rect(r)
                if r.stroke.width == 2.0
                    && r.stroke.color == egui::Color32::RED
                    && r.fill == egui::Color32::TRANSPARENT =>
            {
                out.push(format!("{:?}", r.rect));
            }
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for cs in shapes {
        walk(&cs.shape, &mut out);
    }
    out
}

fn row(refno: u64, depth: u16, name: &str, noun: &str, expandable: Option<bool>) -> TreeRowVm {
    TreeRowVm {
        refno: RefU64(refno),
        depth,
        name: name.to_owned(),
        noun: noun.to_owned(),
        expandable,
        loading: false,
        visibility: RowVisibility::Unloaded,
        next_visible: true,
    }
}

/// 一棵**整个放得进视口**的小树：SITE + 一个可展开的 EQUI + 若干兄弟。
///
/// 放得进是这条用例的前提：展开只让下方的行往下挪、不把谁挤出虚拟滚动的窗口，
/// 于是「旧 id 全都还在」，egui 那条启发式就不该报。挤出视口的那几行是另一回事
/// ——它们本来就不画了，红框躲不掉，也不是这里要守的性质。
fn tree_rows(kids: Option<u64>) -> Vec<TreeRowVm> {
    let mut rows = vec![
        row(1, 0, "/1CUP-HVACHB", "SITE", Some(true)),
        row(2, 1, "/-RX-CUP-EQUI", "ZONE", Some(true)),
        row(3, 2, "/-RX-CUP-001FA", "EQUI", Some(kids.is_some())),
    ];
    if let Some(n) = kids {
        for k in 0..n {
            rows.push(row(900 + k, 3, &format!("BOX {k}"), "BOX", None));
        }
    }
    for i in 0..12u64 {
        rows.push(row(100 + i, 2, &format!("/-RX-CUP-{i:03}ZV"), "EQUI", None));
    }
    rows
}

fn vm_with(rows: Vec<TreeRowVm>) -> WorkbenchVm {
    WorkbenchVm {
        tree: TreeVm::Ready(rows),
        ..Default::default()
    }
}

/// 展开一个节点，下方的行只是挪位置——不该有任何一行换 id。
///
/// 只守展开这一侧。折叠是另一回事：被收掉的那些行**真的不画了**，它们的 id 必然从这一层
/// 消失，egui 照样会在它们原来的位置描红框；那不是 id 不稳，换什么方案都躲不掉。
#[test]
fn row_ids_survive_expanding_a_node() {
    let ctx = Context::default();
    // 900px / 26px 一行 ≈ 34 行；上面那棵树展开后 28 行，全在视口里。
    let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(360.0, 900.0));
    let collapsed = vm_with(tree_rows(None));
    let expanded = vm_with(tree_rows(Some(12)));

    let mut found: Vec<String> = Vec::new();
    // 前三帧让字体与滚动区状态铺开，之后切展开态。
    for (i, vm) in [
        &collapsed, &collapsed, &collapsed, &expanded, &expanded, &expanded,
    ]
    .into_iter()
    .enumerate()
    {
        let input = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let out = ctx.run_ui(input, |ui| {
            let mut cmds = Vec::new();
            plant_ui::workbench::tree::show(ui, &Tokens::light(), Density::Standard, vm, &mut cmds);
        });
        let boxes = red_boxes(&out.shapes);
        if !boxes.is_empty() {
            found.push(format!("  第 {i} 帧：{} 行换了 id {boxes:?}", boxes.len()));
        }
    }

    assert!(
        found.is_empty(),
        "模型树展开时有行换了 id（egui 会逐行描红框）：\n{}",
        found.join("\n")
    );
}
