//! 装载链路的阶段计时（计划 `model-load-speed-parity.md` T0）。
//!
//! 环境变量 `PLANT_UI_LOAD_TIMING=1` 打开；关着时整条链路上只剩一次 `std::env::var`（缓存在
//! `OnceLock` 里），宿主的 `load_timing` 字段恒 `None`，别的什么都不做。开着时一次显示（点眼睛）
//! 记五个时刻与期间最长一帧，网格全部落定后写一行日志面板（`logs.info`）并打一行到 stderr：
//!
//! 1. **点击**：`set_model_visible` 把 `Req::ModelScopes` 发出去的那一拍（基准，下面都是相对它的毫秒）；
//! 2. **记录首页到**：第一条 `Evt::ModelScopeProgress`（拆成员时收完第一个成员）或第一条
//!    `Evt::ModelScope` 回包——整根取法没有逐页进度，首页 = 末页；
//! 3. **记录末页到**：这次点击的全部目标都回了 `Evt::ModelScope`；
//! 4. **首批实体生成**：view3d `load_models` 把第一批记录变成实体（`MeshLoadProgress::spawned_at`）；
//! 5. **网格全部落定**：`update_mesh_progress` 收口（`MeshLoadProgress::settled_at`）。
//!
//! 五个时刻都取自事件到宿主那一拍（view3d 那两个由回执带时刻过来），不另开通道、不让 view3d 写日志。
//! 这里只是一本账 + 一行字，怎么挂到事件上在 `main.rs`。

use std::sync::OnceLock;

use web_time::{Duration, Instant};

/// `PLANT_UI_LOAD_TIMING` 开了没有。只读一次；浏览器端没有环境变量，恒 `false`。
pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("PLANT_UI_LOAD_TIMING")
            .is_ok_and(|value| matches!(value.trim(), "1" | "true" | "on"))
    })
}

/// 一次显示的计时账。
#[derive(Debug, Clone)]
pub struct LoadTiming {
    clicked: Instant,
    targets: usize,
    first_records: Option<Instant>,
    all_records: Option<Instant>,
    first_spawned: Option<Instant>,
    settled: Option<Instant>,
    /// 这次点击的目标们一共发了几发 `model/records`、收了多少字节、多少条记录（回包带来的数）。
    requests: usize,
    bytes: usize,
    records: usize,
    last_frame: Instant,
    longest_frame: Duration,
    frames: u32,
}

impl LoadTiming {
    /// 点击那一拍开账。
    pub fn start(now: Instant, targets: usize) -> Self {
        Self {
            clicked: now,
            targets,
            first_records: None,
            all_records: None,
            first_spawned: None,
            settled: None,
            requests: 0,
            bytes: 0,
            records: 0,
            last_frame: now,
            longest_frame: Duration::ZERO,
            frames: 0,
        }
    }

    /// 记录层有了第一个回音（拆成员时收完第一个成员）。
    pub fn note_records_progress(&mut self, now: Instant) {
        self.first_records.get_or_insert(now);
    }

    /// 一个目标的记录回包到了：`pending_left` 是这次点击还没回的目标数，为 0 就是末页到。
    pub fn note_scope(
        &mut self,
        now: Instant,
        requests: usize,
        bytes: usize,
        records: usize,
        pending_left: usize,
    ) {
        self.first_records.get_or_insert(now);
        self.requests += requests;
        self.bytes += bytes;
        self.records += records;
        if pending_left == 0 {
            self.all_records.get_or_insert(now);
        }
    }

    /// view3d 的一拍网格进度：生成时刻只认点击之后的第一批，落定时刻只认点击之后的。
    pub fn note_mesh_progress(&mut self, spawned_at: Option<Instant>, settled_at: Option<Instant>) {
        if let Some(spawned) = spawned_at
            && spawned >= self.clicked
        {
            self.first_spawned.get_or_insert(spawned);
        }
        if let Some(settled) = settled_at
            && settled >= self.clicked
        {
            self.settled = Some(settled);
        }
    }

    /// 每帧一次：量帧间隔，记最长的那一帧。
    pub fn tick_frame(&mut self, now: Instant) {
        let frame = now.saturating_duration_since(self.last_frame);
        if frame > self.longest_frame {
            self.longest_frame = frame;
        }
        self.last_frame = now;
        self.frames += 1;
    }

    /// 日志面板那一行。没到的时刻写「—」，一眼看出链路停在哪一段。
    pub fn summary(&self) -> String {
        let since = |at: Option<Instant>| match at {
            Some(at) => format!(
                "+{} ms",
                at.saturating_duration_since(self.clicked).as_millis()
            ),
            None => "—".to_owned(),
        };
        format!(
            "装载计时：{} 个目标 · 记录首页 {} · 记录末页 {}（{} 发 / {} KB / {} 条） · 首批实体 {} · 网格落定 {} · 期间最长一帧 {} ms（{} 帧）",
            self.targets,
            since(self.first_records),
            since(self.all_records),
            self.requests,
            self.bytes / 1024,
            self.records,
            since(self.first_spawned),
            since(self.settled),
            self.longest_frame.as_millis(),
            self.frames,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 五个时刻按链路顺序落账，汇总行按相对点击的毫秒写；记录层的发数 / 字节 / 条数按目标累加，
    /// 末页只在最后一个目标回包时记。
    #[test]
    fn a_full_load_lands_all_five_marks_in_order() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut timing = LoadTiming::start(t0, 2);
        timing.tick_frame(at(16));
        timing.note_records_progress(at(40));
        timing.tick_frame(at(90));
        timing.note_scope(at(120), 3, 4096, 50, 1);
        assert!(timing.all_records.is_none(), "还有目标没回，不算末页");
        timing.note_scope(at(200), 2, 2048, 20, 0);
        timing.note_mesh_progress(Some(at(230)), None);
        timing.tick_frame(at(240));
        timing.note_mesh_progress(Some(at(230)), None);
        timing.note_mesh_progress(Some(at(230)), Some(at(900)));
        timing.tick_frame(at(260));

        assert_eq!(timing.first_records, Some(at(40)));
        assert_eq!(timing.all_records, Some(at(200)));
        assert_eq!(timing.first_spawned, Some(at(230)));
        assert_eq!(timing.settled, Some(at(900)));
        assert_eq!(timing.longest_frame, Duration::from_millis(150));
        assert_eq!(timing.frames, 4);
        assert_eq!(
            timing.summary(),
            "装载计时：2 个目标 · 记录首页 +40 ms · 记录末页 +200 ms（5 发 / 6 KB / 70 条） · 首批实体 +230 ms · 网格落定 +900 ms · 期间最长一帧 150 ms（4 帧）"
        );
    }

    /// 整根取法没有逐页进度：首页 = 末页（第一条回包同时落两个时刻）。查空 / 失败没有实体与网格：
    /// 后两个时刻写「—」。
    #[test]
    fn a_whole_root_load_has_first_page_equal_to_last_and_missing_marks_show_as_dashes() {
        let t0 = Instant::now();
        let mut timing = LoadTiming::start(t0, 1);
        timing.note_scope(t0 + Duration::from_millis(300), 1, 0, 0, 0);
        assert_eq!(timing.first_records, timing.all_records);
        let line = timing.summary();
        assert!(
            line.contains("记录首页 +300 ms · 记录末页 +300 ms（1 发 / 0 KB / 0 条）"),
            "{line}"
        );
        assert!(line.contains("首批实体 — · 网格落定 —"), "{line}");
    }

    /// 点击之前的那一批（上一次显示留下的进度）不算这次的：生成 / 落定时刻都只认点击之后的。
    #[test]
    fn marks_from_before_the_click_are_ignored() {
        let t0 = Instant::now();
        let before = t0.checked_sub(Duration::from_millis(500)).unwrap_or(t0);
        let mut timing = LoadTiming::start(t0 + Duration::from_millis(1), 1);
        timing.note_mesh_progress(Some(before), Some(before));
        assert!(timing.first_spawned.is_none());
        assert!(timing.settled.is_none());
    }

    /// 开关只认 `1` / `true` / `on`；没设就是关。读的是进程环境，这里只钉解析规则。
    #[test]
    fn the_switch_is_read_once_and_defaults_to_off() {
        assert_eq!(enabled(), enabled(), "缓存住的值每次一样");
        let parse = |value: &str| matches!(value.trim(), "1" | "true" | "on");
        assert!(parse("1") && parse(" true ") && parse("on"));
        assert!(!parse("0") && !parse("") && !parse("yes"));
    }
}
