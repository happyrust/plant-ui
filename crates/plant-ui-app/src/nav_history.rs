//! 导航历史栈：命令栏最左那两枚箭头背后的账本（S1-D）。
//!
//! 一条历史 = **选择集 + 相机位姿 + 激活页签（+ 房间聚焦）**。它记的是「上一次操作的
//! 地方」，所以相机那一格存的是**离开那一刻**的位姿：人选中 A 之后转了半圈、拉近看，
//! 再点 B——回到 A 时要的是走之前看到的那一幅，不是刚点中 A 时的。做法是每次入栈 /
//! 后退 / 前进之前先把当前相机写回当前条目。
//!
//! 浏览器语义：站在栈中间做新操作，前向分支截断。纯转视角不入栈（否则每拖一下都进
//! 一条），相机只是条目的一个属性。上限 [`CAP`]，满了丢最老的。
//!
//! 这里只有纯逻辑，不认识 Vm 也不认识视口——回放做什么（选中 / 树定位 / 页签 / 相机）
//! 由宿主拿着返回的条目去做；回放期间宿主用 [`NavHistory::navigating`] 挡住再入栈。

use plant_ui::vm::Selection;
use plant_ui::workbench::Pane;
use plant_ui::{CameraPose, NavStep};
use plant_ui_data::RefU64;

/// 栈上限。够回溯一上午的活，又不至于让右键列表无从下手。
pub const CAP: usize = 50;

#[derive(Debug, Clone, PartialEq)]
pub struct NavEntry {
    pub selection: Selection,
    /// 记录那一刻有焦点的 dock 页签；`None` = 还没人点过 dock，回放时不动页签。
    pub pane: Option<Pane>,
    /// 「房间」页签当时聚焦的房间。回放只还详情，不重做隔离 / 取景——那一下会把
    /// 记下的相机顶掉。
    pub room: Option<RefU64>,
    /// 离开这一条时的相机；刚入栈还没离开过时是入栈那一刻的。独立壳没有相机时为 `None`。
    pub camera: Option<CameraPose>,
}

#[derive(Debug, Default)]
pub struct NavHistory {
    entries: Vec<NavEntry>,
    cursor: Option<usize>,
    /// 回放进行中：这期间宿主的 `set_selection` / `focus_room` 照常跑，但不入栈。
    pub navigating: bool,
}

impl NavHistory {
    pub fn entries(&self) -> &[NavEntry] {
        &self.entries
    }

    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    /// 一次新的导航落地。`leaving_camera` 是此刻的相机，先写回当前条目再推新的。
    ///
    /// 与当前条目同一处（选择集与房间都相同）就不记——树上点两下同一行不该占两格；
    /// 回放期间（`navigating`）也不记。站在栈中间时前向分支截断。
    pub fn record(&mut self, entry: NavEntry, leaving_camera: Option<CameraPose>) {
        if self.navigating {
            return;
        }
        if let Some(current) = self.current_mut() {
            if current.selection == entry.selection && current.room == entry.room {
                return;
            }
            if leaving_camera.is_some() {
                current.camera = leaving_camera;
            }
        }
        if let Some(cursor) = self.cursor {
            self.entries.truncate(cursor + 1);
        }
        self.entries.push(entry);
        if self.entries.len() > CAP {
            let overflow = self.entries.len() - CAP;
            self.entries.drain(..overflow);
        }
        self.cursor = Some(self.entries.len() - 1);
    }

    /// 走一步。先把此刻的相机写回当前条目（前进也要能回到刚才的画面），再移游标，
    /// 交出目标条目的副本；走不动（栈空 / 到头 / 下标越界 / 原地）时 `None`。
    pub fn step(&mut self, step: NavStep, leaving_camera: Option<CameraPose>) -> Option<NavEntry> {
        let cursor = self.cursor?;
        let target = match step {
            NavStep::Back => cursor.checked_sub(1)?,
            NavStep::Forward => cursor + 1,
            NavStep::Jump(i) => i,
        };
        if target == cursor || target >= self.entries.len() {
            return None;
        }
        if leaving_camera.is_some() {
            self.entries[cursor].camera = leaving_camera;
        }
        self.cursor = Some(target);
        Some(self.entries[target].clone())
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.cursor = None;
        self.navigating = false;
    }

    fn current_mut(&mut self) -> Option<&mut NavEntry> {
        let cursor = self.cursor?;
        self.entries.get_mut(cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(refno: u64) -> NavEntry {
        NavEntry {
            selection: Selection::single(RefU64(refno)),
            pane: Some(Pane::Properties),
            room: None,
            camera: None,
        }
    }

    fn cam(x: f32) -> Option<CameraPose> {
        Some(CameraPose {
            position: [x, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            focus: [0.0; 3],
        })
    }

    fn primary(entry: &NavEntry) -> u64 {
        entry.selection.primary().unwrap().0
    }

    /// 后退 / 前进是浏览器语义：后退之后做新操作，前向分支被截断。
    #[test]
    fn a_new_navigation_after_going_back_truncates_the_forward_branch() {
        let mut nav = NavHistory::default();
        for r in 1..=3 {
            nav.record(entry(r), None);
        }
        assert_eq!(primary(&nav.step(NavStep::Back, None).unwrap()), 2);
        assert_eq!(primary(&nav.step(NavStep::Back, None).unwrap()), 1);
        assert!(nav.step(NavStep::Back, None).is_none(), "到底了");
        assert_eq!(primary(&nav.step(NavStep::Forward, None).unwrap()), 2);

        nav.record(entry(9), None);
        let refnos: Vec<u64> = nav.entries().iter().map(primary).collect();
        assert_eq!(refnos, vec![1, 2, 9], "3 被截掉");
        assert_eq!(nav.cursor(), Some(2));
        assert!(nav.step(NavStep::Forward, None).is_none());
    }

    /// 相机记的是**离开那一刻**：入栈 / 后退 / 前进之前都把此刻的相机写回当前条目。
    /// 于是回到 A 看到的是走之前的机位，前进回 B 也是刚才的机位。
    #[test]
    fn the_camera_of_an_entry_is_where_you_left_it() {
        let mut nav = NavHistory::default();
        nav.record(entry(1), cam(0.0));
        // 在 1 上转了转相机，再去 2：1 的相机应更新成离开时的 5.0。
        nav.record(entry(2), cam(5.0));
        assert_eq!(nav.entries()[0].camera, cam(5.0));
        // 在 2 上又动了相机，后退：2 存下 7.0，回放拿到的是 1 的 5.0。
        let back = nav.step(NavStep::Back, cam(7.0)).unwrap();
        assert_eq!(back.camera, cam(5.0));
        assert_eq!(nav.entries()[1].camera, cam(7.0));
        // 前进回 2 拿到的是 7.0，不是当初入栈时的。
        let fwd = nav.step(NavStep::Forward, cam(5.5)).unwrap();
        assert_eq!(fwd.camera, cam(7.0));
    }

    /// 同一处不重复入栈；回放期间不入栈——否则一次后退就把自己又推回栈顶，
    /// 「前进」永远灰着。
    #[test]
    fn duplicates_and_replayed_selections_do_not_enter_the_stack() {
        let mut nav = NavHistory::default();
        nav.record(entry(1), None);
        nav.record(entry(1), None);
        assert_eq!(nav.entries().len(), 1);
        nav.record(entry(2), None);
        nav.step(NavStep::Back, None);
        nav.navigating = true;
        nav.record(entry(1), None);
        nav.navigating = false;
        assert_eq!(nav.entries().len(), 2);
        assert_eq!(nav.cursor(), Some(0));
        // 同一元素换了房间聚焦算另一处。
        let mut roomed = entry(1);
        roomed.room = Some(RefU64(77));
        nav.record(roomed, None);
        assert_eq!(nav.entries().len(), 2, "前向的 2 被截掉、房间那条顶上");
        assert_eq!(nav.entries()[1].room, Some(RefU64(77)));
    }

    /// 上限之上丢最老的，游标仍指着最新一条；`Jump` 越界或原地都是无操作。
    #[test]
    fn the_stack_is_capped_and_jump_is_bounds_checked() {
        let mut nav = NavHistory::default();
        for r in 1..=(CAP as u64 + 5) {
            nav.record(entry(r), None);
        }
        assert_eq!(nav.entries().len(), CAP);
        assert_eq!(primary(&nav.entries()[0]), 6);
        assert_eq!(nav.cursor(), Some(CAP - 1));
        assert!(nav.step(NavStep::Jump(CAP), None).is_none());
        assert!(nav.step(NavStep::Jump(CAP - 1), None).is_none(), "原地");
        assert_eq!(primary(&nav.step(NavStep::Jump(0), None).unwrap()), 6);
        nav.clear();
        assert!(nav.step(NavStep::Forward, None).is_none());
        assert!(nav.cursor().is_none());
    }
}
