//! App 侧的日志缓冲：给每行打上本地时间戳与级别、限长、记住错误行能重做什么。
//!
//! 级别在**发日志的这一刻**就定死，绘制层拿到的是已分好级的行。旧壳只能拿一个
//! 字符串去猜级别，词表照真实语料校准过仍会错判；这里没有猜的环节。

use std::collections::HashMap;

use plant_ui::vm::{LogCounts, LogElement, LogLevel, LogLineVm, LogsVm};
use plant_ui_data::RefU64;

/// 一条错误行对应的可重做操作。行尾「重试」按行号回指它。
#[derive(Debug, Clone, Copy)]
pub enum Retry {
    Connect,
    Children(RefU64),
    Props(RefU64),
}

/// 缓冲上限。日志是给人看的，留最近这些足够；再多只会拖慢筛选与滚动。
const CAP: usize = 2000;

/// 错误链的显示文本，同一句话只说一遍。
///
/// anyhow 的 `{:#}` 把整条链用 ": " 串起来，但外层错误的 Display 常常已经把 source
/// 的原文抄了进去（thiserror 的 `#[error("...{0}")]` 就是这个形状），于是同一句话在
/// 链上出现两遍。实测数据源连不上时那句 WS 错误正好重复一次，在模型树面板上占掉四行。
///
/// 不能改用 `{}` 了事：那样只留最外层，外层信息笼统而真因在 source 里的时候就把话
/// 说没了。这里按「已经说过的不再说、说得更全的顶掉说得少的」压一遍，既去重也不丢。
pub fn error_chain(err: &anyhow::Error) -> String {
    let mut out = String::new();
    for cause in err.chain() {
        let text = cause.to_string();
        if out.contains(&text) {
            continue;
        }
        // 空串被任何串包含，所以第一轮天然走这条分支。
        if text.contains(&out) {
            out = text;
        } else {
            out.push_str(": ");
            out.push_str(&text);
        }
    }
    out
}

#[derive(Default)]
pub struct LogBuffer {
    next_id: u64,
    /// 行号 -> 可重做操作。只有错误行进这里，跟着行一起被限长丢弃。
    retries: HashMap<u64, Retry>,
}

impl LogBuffer {
    pub fn info(&mut self, vm: &mut LogsVm, message: impl Into<String>) {
        self.push(vm, LogLevel::Info, None, message.into(), None, None);
    }

    pub fn warn(&mut self, vm: &mut LogsVm, message: impl Into<String>) {
        self.push(vm, LogLevel::Warn, None, message.into(), None, None);
    }

    /// 错误行：摘要上屏、完整错误链进 detail（hover 看、行尾「复制」抄走）。
    /// 给得出重做操作的就带上，行尾会多出「重试」。
    pub fn error(
        &mut self,
        vm: &mut LogsVm,
        message: impl Into<String>,
        err: &anyhow::Error,
        retry: Option<Retry>,
    ) {
        let detail = error_chain(err);
        self.push(
            vm,
            LogLevel::Error,
            None,
            message.into(),
            Some(detail),
            retry,
        );
    }

    /// 带元素的三种：正文前多一段可点的元素名，点它定位到树与属性（M1-6）。
    /// 因此正文里不要再重复元素名。
    pub fn info_of(&mut self, vm: &mut LogsVm, el: LogElement, message: impl Into<String>) {
        self.push(vm, LogLevel::Info, Some(el), message.into(), None, None);
    }

    pub fn warn_of(&mut self, vm: &mut LogsVm, el: LogElement, message: impl Into<String>) {
        self.push(vm, LogLevel::Warn, Some(el), message.into(), None, None);
    }

    pub fn error_of(
        &mut self,
        vm: &mut LogsVm,
        el: LogElement,
        message: impl Into<String>,
        err: &anyhow::Error,
        retry: Option<Retry>,
    ) {
        let detail = error_chain(err);
        self.push(
            vm,
            LogLevel::Error,
            Some(el),
            message.into(),
            Some(detail),
            retry,
        );
    }

    pub fn retry_of(&self, id: u64) -> Option<Retry> {
        self.retries.get(&id).copied()
    }

    pub fn clear(&mut self, vm: &mut LogsVm) {
        vm.lines.clear();
        vm.counts = LogCounts::default();
        self.retries.clear();
    }

    fn push(
        &mut self,
        vm: &mut LogsVm,
        level: LogLevel,
        element: Option<LogElement>,
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
            element,
            message,
            detail,
            retryable: retry.is_some(),
        });
        *count_of(&mut vm.counts, level) += 1;
        self.trim(vm);
    }

    /// 超出上限就整批丢掉最早的四分之一，而不是每来一行搬一次整张表。
    fn trim(&mut self, vm: &mut LogsVm) {
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
