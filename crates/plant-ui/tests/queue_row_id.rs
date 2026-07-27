//! 任务队列的行 id 必须只由 task_id 决定，不能跟着行序走。
//!
//! 这个列表会重排：运行中置顶、行随批次进出、每 2 秒一份新快照。而 egui 的点击是
//! 「按下那一帧记 id、松开那一帧按 id 兑现」——行 id 若是自动号（等于「第几行」），
//! 按与松之间夹进的那次重排就会把这一下交给**另一行**。展开错行只是烦，行内那枚
//! 「重试」按错是服务端真的去重跑一遍生成。
//!
//! 与 `tree_id_clash.rs` 是同一条教训的两处兑现，判据不同：模型树那条数的是
//! `warn_if_rect_changes_id` 的红框（同一矩形换了 id），这里数不出来——**位置号恰好
//! 不换 id**，红框那条启发式对它无感。所以这里直接按 id 取行：id 是契约，
//! `Id::new(("task-queue-row", task_id))` 取不到东西，就说明行号又跟着位置走了。

use egui::{Context, Id, RawInput, Rect, pos2, vec2};
use plant_ui::style::tokens::{Density, Tokens};
use plant_ui::task_queue::{KIND_DATA_BATCH, QueueRow, QueueSnapshot, State, TaskEntry, Vm};

const PROJECT: &str = "ProjAMS";

fn queue_row(task_id: &str, dbnum: u32, state: &str) -> QueueRow {
    QueueRow {
        task_id: task_id.into(),
        dbnum,
        db_type: "DESI".into(),
        state: state.into(),
        start_sesno: 1024,
        end_sesno: 1038,
    }
}

fn task(task_id: &str, dbnum: u32, state: &str) -> TaskEntry {
    TaskEntry {
        task_id: task_id.into(),
        kind: KIND_DATA_BATCH.into(),
        state: state.into(),
        project: PROJECT.into(),
        created_at: "2026-07-27T10:00:00+08:00".into(),
        started_at: Some("2026-07-27T10:01:00+08:00".into()),
        dbnum: Some(dbnum),
        db_type: Some("DESI".into()),
        start_sesno: Some(1024),
        end_sesno: Some(1038),
        ..Default::default()
    }
}

/// 三行：db2 / db3 / db4。`running` 指名哪一行在跑（它会被置顶）。
fn vm(running: Option<&str>) -> Vm {
    let rows = ["t-2", "t-3", "t-4"]
        .iter()
        .zip([2u32, 3, 4])
        .map(|(id, dbnum)| {
            let state = if running == Some(*id) { "running" } else { "queued" };
            queue_row(id, dbnum, state)
        })
        .collect();
    let tasks = ["t-2", "t-3", "t-4"]
        .iter()
        .zip([2u32, 3, 4])
        .map(|(id, dbnum)| {
            let state = if running == Some(*id) { "running" } else { "queued" };
            task(id, dbnum, state)
        })
        .collect();
    Vm {
        project: PROJECT.into(),
        queue: QueueSnapshot {
            paused: false,
            rows,
        },
        tasks,
        loaded: true,
        ..Default::default()
    }
}

/// 画几帧，返回每一行按 id 取到的矩形。取不到就是 `None`。
fn render(ctx: &Context, state: &mut State, model: &Vm, frames: usize) -> Vec<Option<Rect>> {
    let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0));
    for _ in 0..frames {
        let input = RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            let mut cmds = Vec::new();
            plant_ui::task_queue::show(
                ui,
                &Tokens::light(),
                Density::Standard,
                model,
                state,
                &mut cmds,
            );
        });
    }
    ["t-2", "t-3", "t-4"]
        .iter()
        .map(|id| {
            ctx.read_response(Id::new(("task-queue-row", *id)))
                .map(|r| r.rect)
        })
        .collect()
}

/// 一行开跑之后置顶，其余行跟着挪——但每一行的 id 必须还指着原来那一行。
#[test]
fn row_ids_follow_the_task_not_the_position() {
    let ctx = Context::default();
    let mut state = State::default();

    // 前几帧让字体与滚动区状态铺开。
    let before = render(&ctx, &mut state, &vm(None), 3);
    assert!(
        before.iter().all(Option::is_some),
        "三行都该按 task_id 取得到；取不到就是行号又跟着位置走了：{before:?}"
    );
    let before: Vec<Rect> = before.into_iter().flatten().collect();
    assert!(
        before[0].top() < before[1].top() && before[1].top() < before[2].top(),
        "排队按 FIFO，t-2 / t-3 / t-4 自上而下"
    );

    // db4 开跑：它被置顶，另外两行整体下移一格。
    let after = render(&ctx, &mut state, &vm(Some("t-4")), 3);
    assert!(
        after.iter().all(Option::is_some),
        "重排之后三行仍该各自取得到：{after:?}"
    );
    let after: Vec<Rect> = after.into_iter().flatten().collect();
    assert!(
        after[2].top() < after[0].top() && after[2].top() < after[1].top(),
        "运行中那一行要置顶，且拿到的仍是 t-4 自己的矩形而不是原位置上那一行的"
    );
    assert!(
        after[0].top() < after[1].top(),
        "剩下两行的先后不变"
    );
    // 位置号那一版在这里也能「过」——因为它压根取不到 id，上面第一条断言就先红了。
    assert_ne!(
        before[2], after[2],
        "t-4 确实换了位置，这一轮重排是真发生了"
    );
}
