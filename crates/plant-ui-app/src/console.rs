//! App 侧的命令行缓冲：给每行打上本地时间戳与级别、限长、记住错误行能重做什么。
//!
//! 级别在**发日志的这一刻**就定死，绘制层拿到的是已分好级的行。旧壳只能拿一个
//! 字符串去猜级别，词表照真实语料校准过仍会错判；这里没有猜的环节。

use std::collections::HashMap;

use plant_ui::vm::{ConsoleVm, LogCounts, LogLevel, LogLineVm};
use plant_ui_data::RefU64;

/// 一条错误行对应的可重做操作。行尾「重试」按行号回指它。
#[derive(Debug, Clone, Copy)]
pub enum Retry {
    Connect,
    Children(RefU64),
    Props(RefU64),
}

/// 缓冲上限。命令行是给人看的，留最近这些足够；再多只会拖慢筛选与滚动。
const CAP: usize = 2000;

#[derive(Default)]
pub struct Console {
    next_id: u64,
    /// 行号 -> 可重做操作。只有错误行进这里，跟着行一起被限长丢弃。
    retries: HashMap<u64, Retry>,
}

impl Console {
    pub fn info(&mut self, vm: &mut ConsoleVm, message: impl Into<String>) {
        self.push(vm, LogLevel::Info, message.into(), None, None);
    }

    pub fn warn(&mut self, vm: &mut ConsoleVm, message: impl Into<String>) {
        self.push(vm, LogLevel::Warn, message.into(), None, None);
    }

    /// 错误行：摘要上屏、完整错误链进 detail（hover 看、行尾「复制」抄走）。
    /// 给得出重做操作的就带上，行尾会多出「重试」。
    pub fn error(
        &mut self,
        vm: &mut ConsoleVm,
        message: impl Into<String>,
        err: &anyhow::Error,
        retry: Option<Retry>,
    ) {
        let detail = format!("{err:#}");
        self.push(vm, LogLevel::Error, message.into(), Some(detail), retry);
    }

    pub fn retry_of(&self, id: u64) -> Option<Retry> {
        self.retries.get(&id).copied()
    }

    pub fn clear(&mut self, vm: &mut ConsoleVm) {
        vm.lines.clear();
        vm.counts = LogCounts::default();
        self.retries.clear();
    }

    fn push(
        &mut self,
        vm: &mut ConsoleVm,
        level: LogLevel,
        message: String,
        detail: Option<String>,
        retry: Option<Retry>,
    ) {
        let id = self.next_id;
        self.next_id += 1;
        if let Some(retry) = retry {
            self.retries.insert(id, retry);
        }
        vm.lines.push(LogLineVm {
            id,
            time: chrono::Local::now().format("%H:%M:%S").to_string(),
            level,
            message,
            detail,
            retryable: retry.is_some(),
        });
        *count_of(&mut vm.counts, level) += 1;
        self.trim(vm);
    }

    /// 超出上限就整批丢掉最早的四分之一，而不是每来一行搬一次整张表。
    fn trim(&mut self, vm: &mut ConsoleVm) {
        if vm.lines.len() <= CAP {
            return;
        }
        for line in vm.lines.drain(..CAP / 4) {
            self.retries.remove(&line.id);
            *count_of(&mut vm.counts, line.level) -= 1;
        }
    }
}

fn count_of(counts: &mut LogCounts, level: LogLevel) -> &mut usize {
    match level {
        LogLevel::Info => &mut counts.info,
        LogLevel::Warn => &mut counts.warn,
        LogLevel::Error => &mut counts.error,
    }
}
