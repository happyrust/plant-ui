//! 标题栏右上角的元素搜索：输入名称或参考号，挑中一条就树定位过去。
//!
//! 两种查法在这里合流，**都跟着按键走**，下拉分成两段：
//!
//! - **名称前缀**落到库里 `pe.name` 的索引范围扫，全库范围，毫秒级。在上一段。
//! - **名称片段**查本地 ngram 索引（ADR-0023），范围是当前 MDB 的设计库，
//!   亚毫秒。在下一段，段首挂一行标注说明范围与上一段不同。
//!
//! 曾经有过一个两段式交互：前缀搜空了才由人按回车去做库内逐行包含匹配（要等
//! 几十秒）。那条路在 ADR-0023 里整个退役了——子串既然已经亚毫秒，就没有理由
//! 让人先撞一次空再按一次回车。
//!
//! 参考号根本不打库：`24381/100819` 这种一眼认得出来的直接交给树定位。

use egui::{
    Align, Align2, Color32, CornerRadius, Id, Key, Layout, Modifiers, PopupCloseBehavior, Rect,
    RichText, Sense, Stroke, TextEdit, Ui, UiBuilder, pos2, vec2,
};
use egui_phosphor::regular as ph;

use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Tokens, radius};
use crate::vm::{SearchHitVm, SubIndexVm, WorkbenchVm};
use crate::{Cmd, RefU64};

/// 搜索框的绘制状态：输入串与下拉的高亮行。不进 Vm——它们是界面自己的事，
/// 与查询结果无关。
#[derive(Default)]
pub struct State {
    input: String,
    /// 下拉里高亮的那一行；输入一变就回到第一行。
    cursor: usize,
    open: bool,
    /// 上一次交出去的查询串。绘制层每帧都在跑，没有它同一个串会每帧发一遍。
    sent: String,
}

impl State {
    fn close(&mut self) {
        self.input.clear();
        self.sent.clear();
        self.cursor = 0;
        self.open = false;
    }
}

/// 输入框里这一串是什么。
#[derive(Debug, Clone, PartialEq, Eq)]
enum Query {
    Empty,
    /// 长得像参考号：不打库，回车直接定位。
    Ref(RefU64),
    /// 按名称查（原样交给宿主，前导 `/` 与大小写由数据层补齐）。
    Name(String),
}

/// 名称与参考号的分界。
///
/// 带前导 `/` 的一律是名称——PDMS 的名字都带它，参考号不带。这一条必须排在
/// 参考号解析前面：`/1516/100` 这种名字在 `RefU64` 眼里是个合法参考号。
///
/// 剩下的交给 `RefU64` 解析，但**纯数字要大过 u32 才算参考号**：`24381` 既可能
/// 是打了一半的参考号，也可能是名字的一截，而 `24381/100819` 不会有第二种读法。
/// 这条口径与命令行 `looks_like_ref` 是同一条——同一串在两个入口不该有两种答案。
fn classify(input: &str) -> Query {
    let input = input.trim();
    if input.is_empty() {
        return Query::Empty;
    }
    if input.starts_with('/') {
        return Query::Name(input.to_owned());
    }
    let looks_like_ref = input.contains(['/', '_', '=', ':'])
        || input
            .parse::<u64>()
            .is_ok_and(|packed| packed > u32::MAX as u64);
    match input.parse::<RefU64>() {
        Ok(refno) if looks_like_ref => Query::Ref(refno),
        _ => Query::Name(input.to_owned()),
    }
}

/// 下拉里的一行。
enum Row<'a> {
    /// 「定位参考号 …」：输入自己就是答案，不必等库。
    Ref(RefU64),
    Hit(&'a SearchHitVm),
}

impl Row<'_> {
    fn refno(&self) -> RefU64 {
        match self {
            Row::Ref(refno) => *refno,
            Row::Hit(hit) => hit.refno,
        }
    }
}

enum Hotkey {
    Move(i8),
    Submit,
    Close,
}

/// 下拉的宽度（基准密度下的点数）。
///
/// 得由内容自己钉住一份，`Popup::width` 只管得着首帧：弹层此后的宽度取自内容的
/// `min_rect`，而每一行的宽度又取自弹层给的可用宽。这个环中间只要有一帧什么都
/// 没画就锁死在 0，此后每一行都是零宽的——参考号贴到框的左边、名字被挤成几个
/// 字、整片文字漏到弹层外面盖在三维视图上——而且自己长不回来。
const DROPDOWN_WIDTH: f32 = 420.0;

pub fn search_box(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    vm: &WorkbenchVm,
    state: &mut State,
    cmds: &mut Vec<Cmd>,
) {
    let (rect, _) = ui.allocate_exact_size(vec2(d.px(320.0), d.px(26.0)), Sense::hover());
    let id = Id::new("chrome-search-input");
    let focused = ui.memory(|memory| memory.has_focus(id));

    // 键盘要抢在 TextEdit 前面处理：单行输入自己会吃掉上下键（移光标）与回车
    // （失焦提交），等它处理完再问就什么都不剩了。
    let mut hotkey = None;
    if focused {
        ui.input_mut(|i| {
            if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                hotkey = Some(Hotkey::Move(1));
            } else if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                hotkey = Some(Hotkey::Move(-1));
            } else if i.consume_key(Modifiers::NONE, Key::Enter) {
                hotkey = Some(Hotkey::Submit);
            } else if i.consume_key(Modifiers::NONE, Key::Escape) {
                hotkey = Some(Hotkey::Close);
            }
        });
    }
    let summoned = ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::K));

    let text_rect = frame_and_text_rect(ui, t, d, rect, focused, state.input.is_empty());
    let mut field = ui.new_child(
        UiBuilder::new()
            .max_rect(text_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let response = field.add(
        TextEdit::singleline(&mut state.input)
            .id(id)
            // 框体由 `frame_and_text_rect` 自绘，这里只要一块透明的文字区。
            .frame(egui::Frame::NONE)
            .background_color(Color32::TRANSPARENT)
            .desired_width(text_rect.width())
            .font(Font::meta(d))
            .text_color(t.text_primary)
            .hint_text(RichText::new("搜索名称或参考号").color(t.text_muted)),
    );
    if summoned {
        response.request_focus();
    }
    if response.changed() {
        state.cursor = 0;
    }
    if response.changed() || response.gained_focus() || summoned {
        state.open = !state.input.trim().is_empty();
    }

    // 输入一变就发一次查询，两路一起。
    let query = classify(&state.input);
    match &query {
        Query::Name(name) if state.sent != *name => {
            state.sent.clone_from(name);
            cmds.push(Cmd::SearchElements {
                query: name.clone(),
            });
        }
        Query::Empty | Query::Ref(_) if !state.sent.is_empty() => {
            state.sent.clear();
            cmds.push(Cmd::CloseSearch);
        }
        _ => {}
    }

    let (rows, prefix_rows) = rows_of(vm, &query);
    state.cursor = state.cursor.min(rows.len().saturating_sub(1));

    let mut picked = None;
    match hotkey {
        Some(Hotkey::Move(step)) if !rows.is_empty() => {
            state.cursor = moved(state.cursor, step, rows.len());
            state.open = true;
        }
        // 没有可选的行时回车什么都不做：两段式退役之后它不再是任何东西的入口。
        Some(Hotkey::Submit) => {
            if let Some(row) = rows.get(state.cursor) {
                picked = Some(row.refno());
            }
        }
        Some(Hotkey::Close) => {
            state.close();
            cmds.push(Cmd::CloseSearch);
            ui.memory_mut(|memory| memory.surrender_focus(id));
        }
        Some(Hotkey::Move(_)) | None => {}
    }

    if state.open {
        let mut open = true;
        let cursor = state.cursor;
        let clicked = egui::Popup::from_response(&response)
            .id(Id::new("chrome-search-popup"))
            .anchor(rect)
            .open_bool(&mut open)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .gap(d.px(4.0))
            .width(d.px(DROPDOWN_WIDTH))
            .frame(
                egui::Frame::new()
                    .fill(t.bg_elevated)
                    .stroke(Stroke::new(1.0_f32, t.border))
                    .corner_radius(CornerRadius::same(radius::MD))
                    .inner_margin(egui::Margin::symmetric(0, d.px(4.0) as i8))
                    .shadow(ui.style().visuals.popup_shadow),
            )
            .show(|ui| dropdown(ui, t, d, vm, &query, &rows, prefix_rows, cursor))
            .and_then(|inner| inner.inner);
        state.open = open;
        picked = picked.or(clicked);
    }

    if let Some(refno) = picked {
        // 定位走命令行 `/名称` 那条既有链路：选中、展开祖先、把行滚进视野。
        cmds.push(Cmd::LocateElement(refno));
        state.close();
        cmds.push(Cmd::CloseSearch);
        ui.memory_mut(|memory| memory.surrender_focus(id));
    }
}

/// 画框体、放大镜与空框时的 Ctrl K，返回留给文字的那一段。
///
/// 都画在 TextEdit **之前**：同一层里后画的盖住先画的，反过来底色会把文字埋了。
/// 代价是聚焦描边慢一帧（`focused` 取自上一帧的记忆），交互中每帧都在重绘，看不出来。
fn frame_and_text_rect(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    rect: Rect,
    focused: bool,
    empty: bool,
) -> Rect {
    let cr = CornerRadius::same(radius::MD);
    let painter = ui.painter();
    painter.rect_filled(rect, cr, t.bg_input);
    painter.rect_stroke(
        rect,
        cr,
        Stroke::new(1.0_f32, if focused { t.accent } else { t.border }),
        egui::StrokeKind::Inside,
    );
    let pad = d.px(10.0);
    let icon = painter.layout_no_wrap(
        ph::MAGNIFYING_GLASS.to_owned(),
        egui::FontId::new(d.px(13.0), egui::FontFamily::Proportional),
        t.text_muted,
    );
    let icon_w = icon.size().x;
    painter.galley(
        pos2(rect.left() + pad, rect.center().y - icon.size().y / 2.0),
        icon,
        t.text_muted,
    );
    let mut right = rect.right() - pad;
    if empty && !focused {
        let key = painter.layout_no_wrap("Ctrl K".to_owned(), Font::mono_micro(d), t.text_muted);
        let size = key.size();
        painter.galley(
            pos2(right - size.x, rect.center().y - size.y / 2.0),
            key,
            t.text_muted,
        );
        right -= size.x + d.px(8.0);
    }
    Rect::from_min_max(
        pos2(rect.left() + pad + icon_w + d.px(8.0), rect.top()),
        pos2(right, rect.bottom()),
    )
}

/// 手上这份结果确实是当前输入串的结果。
///
/// 结果总要晚一拍回来，那期间 `hits` 还是上一个串的；不对表就会把旧命中挂在
/// 新输入下面。
fn settled(vm: &WorkbenchVm, query: &Query) -> bool {
    matches!(query, Query::Name(name) if vm.search.query == *name)
}

/// 下拉里那些**可选**的行，以及前缀段占了其中前几行。
///
/// 两段拼成一串而不是两个列表：键盘上下要能从前缀段一路穿到子串段，光标那套
/// 环绕逻辑只认一个连续序号。段界只用来插那行标注，不参与选择。
fn rows_of<'a>(vm: &'a WorkbenchVm, query: &Query) -> (Vec<Row<'a>>, usize) {
    match query {
        Query::Ref(refno) => (vec![Row::Ref(*refno)], 1),
        Query::Name(_) if settled(vm, query) => {
            let mut rows: Vec<Row<'a>> = vm.search.hits.iter().map(Row::Hit).collect();
            let prefix_rows = rows.len();
            rows.extend(vm.search.sub_hits.iter().map(Row::Hit));
            (rows, prefix_rows)
        }
        _ => (Vec::new(), 0),
    }
}

/// 光标挪一格，两头环绕。`len` 保证不为 0 由调用方负责。
fn moved(cursor: usize, step: i8, len: usize) -> usize {
    let last = len - 1;
    match step {
        1 if cursor >= last => 0,
        1 => cursor + 1,
        _ if cursor == 0 => last,
        _ => cursor - 1,
    }
}

fn dropdown(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    vm: &WorkbenchVm,
    query: &Query,
    rows: &[Row<'_>],
    prefix_rows: usize,
    cursor: usize,
) -> Option<RefU64> {
    ui.set_min_width(d.px(DROPDOWN_WIDTH));
    ui.spacing_mut().item_spacing.y = 0.0;
    let name = match query {
        Query::Name(name) => Some(name.as_str()),
        _ => None,
    };
    let mut picked = None;
    for (index, row) in rows.iter().enumerate() {
        // 子串段的段首挂一行不可选的标注：这一段的范围与上一段不一样，
        // 不说清楚人会以为搜索框把别的库漏了。
        if let (true, Some(name)) = (index == prefix_rows, name) {
            let scope = vm.search.scope_dbs;
            note(
                ui,
                d,
                &format!("名字中间含「{name}」——{scope} 个设计库（当前 MDB）"),
                t.text_muted,
            );
        }
        if candidate_row(ui, t, d, row, index == cursor) {
            picked = Some(row.refno());
        }
    }
    let Some(name) = name else {
        if matches!(query, Query::Ref(_)) {
            note(ui, d, "回车定位到这个参考号", t.text_muted);
        }
        return picked;
    };
    // 名称查询下候选行与状态行不许同时为空：空内容会把弹层的宽度压成 0，而那是
    // 回不来的（见 `DROPDOWN_WIDTH`）。
    //
    // 所以「在途」认的是结果有没有对上表，而不是 `running` 立没立起来：查询这一
    // 帧才进 `cmds`，宿主下一帧才认领它，中间那一帧两个字段都还空着——照
    // `running` 判，正好就是一行都不画的那一帧。
    if let Some(error) = &vm.search.error {
        note(ui, d, error, t.danger);
    } else if !settled(vm, query) {
        note(ui, d, "搜索中…", t.text_muted);
    } else if rows.is_empty() {
        let scope = vm.search.scope_dbs;
        let miss = match vm.search.sub_state {
            // 子串这一路真的搜过了才敢替它说「也没有」。
            SubIndexVm::Ready => {
                format!("没有名字以「{name}」开头的元素；{scope} 个设计库里也没有名字含它的")
            }
            _ => format!("没有名字以「{name}」开头的元素"),
        };
        note(ui, d, &miss, t.text_secondary);
    } else if vm.search.truncated {
        note(
            ui,
            d,
            "命中太多，只列前几条；多打几个字缩小范围",
            t.text_muted,
        );
    }
    // 子串那一路的近况与前缀有没有命中无关——「先看开头匹配的」正是要在
    // 前缀有命中时也说出来。
    match &vm.search.sub_state {
        SubIndexVm::Building { done, total } => note(
            ui,
            d,
            &format!("子串索引准备中（{done}/{total} 库）…先看开头匹配的"),
            t.text_muted,
        ),
        SubIndexVm::Failed(reason) => note(
            ui,
            d,
            &format!("子串搜索不可用：{reason}（命令行输入 reindex 重建）"),
            t.danger,
        ),
        // 浏览器端没有索引。只在前缀也一无所获时说，免得每次搜索都念一遍。
        SubIndexVm::Off if rows.is_empty() && settled(vm, query) => {
            note(ui, d, "按中间片段搜索仅桌面端提供", t.text_muted)
        }
        SubIndexVm::Off | SubIndexVm::Ready => {}
    }
    picked
}

/// 一条候选。左起类型图标 + 类型 + 全名，右侧参考号；树外元素另挂一枚标。
fn candidate_row(ui: &mut Ui, t: &Tokens, d: Density, row: &Row<'_>, active: bool) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), d.px(28.0)), Sense::click());
    if !ui.is_rect_visible(rect) {
        return response.clicked();
    }
    let painter = ui.painter();
    if active || response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(radius::SM), t.bg_hover);
    }
    let pad = d.px(10.0);
    let cy = rect.center().y;
    let mut x = rect.left() + pad;
    painter.text(
        pos2(x, cy),
        Align2::LEFT_CENTER,
        match row {
            Row::Ref(_) => ph::CROSSHAIR,
            Row::Hit(hit) => super::noun_icon(&hit.noun),
        },
        egui::FontId::new(d.px(13.0), egui::FontFamily::Proportional),
        if active { t.accent } else { t.text_muted },
    );
    x += d.px(20.0);
    if let Row::Hit(hit) = row {
        let noun = painter.text(
            pos2(x, cy),
            Align2::LEFT_CENTER,
            &hit.noun,
            Font::mono_micro(d),
            t.text_muted,
        );
        x = noun.right() + d.px(8.0);
    }
    let refno = painter.text(
        pos2(rect.right() - pad, cy),
        Align2::RIGHT_CENTER,
        row.refno().to_pdms_str(),
        Font::mono_micro(d),
        t.text_muted,
    );
    let mut right = refno.left() - d.px(8.0);
    if matches!(row, Row::Hit(hit) if !hit.in_tree) {
        let tag = painter.text(
            pos2(right, cy),
            Align2::RIGHT_CENTER,
            "树外",
            Font::micro(d),
            t.warn,
        );
        right = tag.left() - d.px(6.0);
    }
    let label = match row {
        Row::Ref(_) => "定位参考号",
        Row::Hit(hit) => hit.name.as_str(),
    };
    painter.text(
        pos2(x, cy),
        Align2::LEFT_CENTER,
        elide_middle(label, (right - x).max(d.px(40.0)), d),
        Font::meta(d),
        if active {
            t.text_primary
        } else {
            t.text_secondary
        },
    );
    response.clicked()
}

fn note(ui: &mut Ui, d: Density, text: &str, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), d.px(22.0)), Sense::hover());
    ui.painter().text(
        pos2(rect.left() + d.px(10.0), rect.center().y),
        Align2::LEFT_CENTER,
        text,
        Font::micro(d),
        color,
    );
}

/// 名字长过给它的那一段就掐中间。**不掐尾巴**：两头都是分辨元素的关键，
/// 掐尾会把 `/C-CO-1RX-210-AB` 与 `/C-CO-1RX-210-CD` 变成同一行。
///
/// 宽度按字符数粗估：真正的排版结果要画完才知道，而候选行只求「看得出是哪一个」。
fn elide_middle(text: &str, width: f32, d: Density) -> String {
    let fits = (width / (d.meta() * 0.62)).floor().max(6.0) as usize;
    let chars = text.chars().count();
    if chars <= fits {
        return text.to_owned();
    }
    let keep = fits.saturating_sub(1);
    let head = keep.div_ceil(2);
    let front: String = text.chars().take(head).collect();
    let back: String = text.chars().skip(chars - (keep - head)).collect();
    format!("{front}…{back}")
}

#[cfg(test)]
mod tests {
    use super::{DROPDOWN_WIDTH, Query, Row, classify, dropdown, elide_middle, moved, rows_of};
    use crate::RefU64;
    use crate::style::tokens::{Density, Tokens};
    use crate::vm::{SearchHitVm, SearchVm, WorkbenchVm};

    fn hit(name: &str, packed: u64) -> SearchHitVm {
        SearchHitVm {
            refno: RefU64::from(packed),
            name: name.to_owned(),
            noun: "EQUI".to_owned(),
            in_tree: true,
        }
    }

    fn vm_with(query: &str, prefix: &[&str], substring: &[&str]) -> WorkbenchVm {
        let mut vm = WorkbenchVm::default();
        vm.search = SearchVm {
            query: query.to_owned(),
            hits: prefix
                .iter()
                .enumerate()
                .map(|(i, name)| hit(name, 100 + i as u64))
                .collect(),
            sub_hits: substring
                .iter()
                .enumerate()
                .map(|(i, name)| hit(name, 200 + i as u64))
                .collect(),
            ..Default::default()
        };
        vm
    }

    /// 两段拼成一串，前缀在前、子串在后，分界报得准——段界只用来插标注，
    /// 键盘要能一路穿过去。
    #[test]
    fn the_two_sections_join_into_one_selectable_run() {
        let vm = vm_with("rs", &["/rs-a", "/rs-b"], &["/x-rs-1"]);
        let (rows, prefix_rows) = rows_of(&vm, &Query::Name("rs".into()));
        assert_eq!(prefix_rows, 2);
        let names: Vec<&str> = rows
            .iter()
            .map(|row| match row {
                Row::Hit(hit) => hit.name.as_str(),
                Row::Ref(_) => "ref",
            })
            .collect();
        assert_eq!(names, vec!["/rs-a", "/rs-b", "/x-rs-1"]);

        // 只有子串命中时段界在 0：标注排在第一行，前缀段整个不出现。
        let vm = vm_with("rs", &[], &["/x-rs-1"]);
        assert_eq!(rows_of(&vm, &Query::Name("rs".into())).1, 0);

        // 结果还是上一个串的（没对上表）就一行都不给——旧命中不许挂在新输入下面。
        let vm = vm_with("rs", &["/rs-a"], &["/x-rs-1"]);
        assert!(rows_of(&vm, &Query::Name("rs-c".into())).0.is_empty());

        // 一行都没有时回车没有任何可发的东西：两段式退役之后它不再是入口。
        let empty = WorkbenchVm::default();
        assert!(rows_of(&empty, &Query::Name("rs".into())).0.is_empty());
    }

    /// 光标在两段拼起来的那一串上环绕，不在段界卡住。
    #[test]
    fn the_cursor_wraps_across_both_sections() {
        // 3 行：0 1 2 属于「前缀 2 条 + 子串 1 条」那一串。
        assert_eq!(moved(0, 1, 3), 1);
        assert_eq!(moved(1, 1, 3), 2, "跨段界照样往下走");
        assert_eq!(moved(2, 1, 3), 0, "末行往下回到头");
        assert_eq!(moved(0, -1, 3), 2, "首行往上绕到尾");
        assert_eq!(moved(2, -1, 3), 1);
        assert_eq!(moved(0, 1, 1), 0, "只有一行时怎么按都是它");
    }

    /// 参考号与名称分得开，是这个框能同时接两种输入的前提。
    #[test]
    fn refnos_and_names_take_different_roads() {
        assert_eq!(
            classify("24381/100819"),
            Query::Ref("24381/100819".parse().unwrap())
        );
        assert_eq!(
            classify("24381_100819"),
            Query::Ref("24381/100819".parse().unwrap())
        );
        assert_eq!(classify("  1RX-210 "), Query::Name("1RX-210".into()));
        assert_eq!(classify("/1RX-210"), Query::Name("/1RX-210".into()));
        // 名字里带斜杠也照样是名字：前导 `/` 那一条先判，不然它会被当成参考号。
        assert_eq!(classify("/1516/100"), Query::Name("/1516/100".into()));
        // 打了一半的参考号还是名字：`24381` 也可能是名字的一截。
        assert_eq!(classify("24381"), Query::Name("24381".into()));
        assert_eq!(classify("   "), Query::Empty);
    }

    /// 下拉自己撑开宽度，就算上一帧把它压成了零宽；名称查询下也总有一行东西。
    ///
    /// 这两件事是同一件：弹层的宽度取自内容的 `min_rect`，而每一行的宽度又取自
    /// 弹层给的可用宽。这个环空过一帧就锁死在 0，此后参考号贴在框的左边、名字
    /// 只剩几个字、整片文字漏到弹层外面盖住三维视图，而且自己长不回来。
    #[test]
    fn the_dropdown_holds_its_width_open_even_before_results_land() {
        let t = Tokens::dark();
        let d = Density::Standard;
        let query = Query::Name("rs".into());
        // 第一份是查询刚进 `cmds`、结果还没对上表的那一帧——正是它把宽度压没的。
        let states = [
            WorkbenchVm::default(),
            vm_with("rs", &["/rs-a"], &["/x-rs"]),
        ];

        for vm in states {
            let (rows, prefix_rows) = rows_of(&vm, &query);
            let mut size = egui::Vec2::ZERO;
            egui::__run_test_ui(|ui| {
                // 上一帧留下的零宽 max_rect：不自己钉宽度就照着它画每一行。
                let squeezed = egui::Rect::from_min_size(ui.max_rect().min, egui::Vec2::ZERO);
                let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(squeezed));
                dropdown(&mut ui, &t, d, &vm, &query, &rows, prefix_rows, 0);
                size = ui.min_rect().size();
            });
            assert_eq!(size.x, d.px(DROPDOWN_WIDTH), "宽度得由内容自己钉住");
            assert!(size.y > 0.0, "一行都不画的那一帧就是宽度归零的那一帧");
        }
    }

    #[test]
    fn long_names_lose_their_middle_not_their_tail() {
        let elided = elide_middle("/C-CO-1RX-210-AB", 40.0, Density::Standard);
        assert!(elided.contains('…'), "{elided}");
        assert!(elided.ends_with("AB"), "{elided}");
        assert_eq!(
            elide_middle("/1RX-210", 400.0, Density::Standard),
            "/1RX-210"
        );
    }
}
