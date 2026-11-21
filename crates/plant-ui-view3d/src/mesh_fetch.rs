//! 网页端取网格文件的调度：同时在途的请求有上限，失败的再试几次，模型服务整个连不上
//! 时不再一个个去撞。
//!
//! 这一层不碰浏览器接口——发请求、等一会儿、看时间都经 [`Transport`]。所以调度的每条
//! 规矩在桌面端的 `cargo test` 里就能跑到；浏览器那一半只剩 `mesh_source` 里对
//! `fetch` / `setTimeout` 的一层薄封装。
//!
//! # 为什么不用 Bevy 自带的 `HttpWasmAssetReader`
//!
//! 2026-11-16 在现场的网页包上实测到两件事：
//!
//! - 它每个资产当场发一个 `fetch`，没有上限：一个 SITE 的网格文件在一两秒内全部发出去
//!   （/1PTU-INST23 是 9977 个）。有一轮 2512 个请求里 312 个被浏览器以
//!   `net::ERR_INSUFFICIENT_RESOURCES` 拒掉，页面少了 309 个网格，之后也不会自己补。
//! - 应答头已经到了、正文读到一半连接断开时，它对读正文的结果 `unwrap()`，整个页面
//!   跟着 panic，只能刷新。
//!
//! 那个读取器在 vendored 的 `bevy_asset` 里；这里另起一条只管网格的通道。
//!
//! # 规矩
//!
//! 1. **名额**：同时在途的请求不超过 [`Policy::concurrency`] 个。名额按「一次请求」
//!    发放，不是按「一个文件」——失败之后等着重试的那段时间不占名额，别的文件照常走。
//! 2. **重试**：压根没得到回答（连接没建起来、正文没读完、超时）与 5xx / 408 / 429
//!    再试，间隔见 [`Policy::backoff`]。404 / 403 与其余 4xx 不重试：再问还是同一个回答。
//! 3. **断路**：「压根没得到回答」连着来、持续了 [`Policy::trip_after`]，其间没有任何
//!    一次请求得到服务的回答，就认定服务连不上——此后 [`Policy::cooldown`] 内轮到的文件
//!    直接判失败，不再发请求。没有这一条，服务停着的时候每个请求都要等到连接失败才算完：
//!    端口当场拒绝的话来得快（本机实测几毫秒），对方不应答就要等到超时
//!    （[`Policy::attempt_timeout`]），几千个文件排下来进度条要卡上几个小时。短于
//!    `trip_after` 的中断靠第 2 条熬过去，一个文件都不丢。
//!
//!    服务答了但报错（5xx）不算「连不上」：那种失败回来得快，重试又不占名额，整批
//!    最多拖一个重试周期；反过来，少数几个文件固定报 500 也不该连累别的文件被断路。

use std::fmt;
use std::future::Future;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use async_lock::Semaphore;

/// 一次请求没成的原因，按「再试有没有用」「服务还在不在」分。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// 服务明确说没有这个文件（404 / 403）。
    NotFound,
    /// 服务给了别的拒绝（其余 4xx 等）；再问还是这个回答。
    Rejected(u16),
    /// 服务答了，但这一下没给（5xx / 408 / 429）：再试可能就给了。
    Busy(u16),
    /// 压根没得到服务的回答：连接没建起来、正文读到一半断了、超时。再试可能就通了；
    /// 一直这样就是服务连不上——断路只数这一种。
    Unreached(String),
}

impl Failure {
    /// 应答的状态码说明这一次没成的话，是哪一种没成；200 返回 `None`，接着读正文。
    pub fn from_status(status: u16) -> Option<Self> {
        match status {
            200 => None,
            403 | 404 => Some(Self::NotFound),
            408 | 429 | 500..=599 => Some(Self::Busy(status)),
            _ => Some(Self::Rejected(status)),
        }
    }
}

/// 调度向外界要的全部东西：发请求、等一会儿、看时间。
pub(crate) trait Transport {
    /// 发一次请求并把正文读完。`timeout` 内没读完要自己掐断，按 [`Failure::Unreached`] 报。
    fn get(&self, url: &str, timeout: Duration) -> impl Future<Output = Result<Vec<u8>, Failure>>;
    /// 等这么久。
    fn sleep(&self, wait: Duration) -> impl Future<Output = ()>;
    /// 单调时钟，起点任意。
    fn now(&self) -> Duration;
}

/// 一个文件最终没取到。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FetchError {
    NotFound,
    Rejected(u16),
    /// 试了 `attempts` 次都没成，`last` 是最后一次的原因。
    Exhausted {
        attempts: u32,
        last: String,
    },
    /// 模型服务连不上，这个文件没有（再）去取；`reason` 是认定连不上时的那次失败。
    Unreachable {
        reason: String,
    },
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "服务上没有这个文件"),
            Self::Rejected(status) => write!(f, "HTTP {status}"),
            Self::Exhausted { attempts, last } => {
                write!(f, "试了 {attempts} 次都没取到，最后一次：{last}")
            }
            Self::Unreachable { reason } => {
                write!(f, "模型服务连不上（{reason}），这个文件没有再去取")
            }
        }
    }
}

/// 调度的几个数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Policy {
    /// 同时在途的请求数上限。
    pub concurrency: usize,
    /// 一次请求从发出到正文读完的时限。只为把卡死的请求从名额上清走，所以给得宽。
    pub attempt_timeout: Duration,
    /// 第 n 次失败之后等多久再试；长度就是最多再试几次。
    ///
    /// 加起来要比 [`Self::trip_after`] 长：中断一开始就失败的那个文件，最后一次重试得
    /// 落在「够得上断路的中断」之外，短中断才能做到一个文件都不丢。
    pub backoff: Vec<Duration>,
    /// 「压根没得到回答」连着来、持续这么久就断路。
    pub trip_after: Duration,
    /// 断路至少要攒够这么多次——一两个卡住的请求不足以说明服务连不上。
    pub trip_failures: u32,
    /// 断路之后多久再放请求出去。
    pub cooldown: Duration,
}

impl Policy {
    /// 并发上限的缺省值。
    pub const DEFAULT_CONCURRENCY: usize = 16;
    /// 并发上限最多调到多少。再大就回到「一次全发出去」的老路上了。
    pub const MAX_CONCURRENCY: usize = 256;

    /// 出厂的那一套；`concurrency` 给了就按它来（夹在 1 与 [`Self::MAX_CONCURRENCY`] 之间）。
    pub fn new(concurrency: Option<usize>) -> Self {
        Self {
            concurrency: concurrency
                .unwrap_or(Self::DEFAULT_CONCURRENCY)
                .clamp(1, Self::MAX_CONCURRENCY),
            attempt_timeout: Duration::from_secs(60),
            backoff: vec![
                Duration::from_millis(400),
                Duration::from_millis(1_200),
                Duration::from_millis(3_000),
            ],
            trip_after: Duration::from_secs(4),
            trip_failures: 8,
            cooldown: Duration::from_secs(5),
        }
    }
}

/// 取文件的那条通道。整个页面共用一个：名额与「服务还通不通」的账都在它身上。
pub(crate) struct Fetcher<T> {
    transport: T,
    policy: Policy,
    slots: Semaphore,
    health: Mutex<Health>,
}

impl<T: Transport> Fetcher<T> {
    pub fn new(transport: T, policy: Policy) -> Self {
        Self {
            slots: Semaphore::new(policy.concurrency.max(1)),
            transport,
            policy,
            health: Mutex::new(Health::default()),
        }
    }

    /// 取一个文件：排队等名额，失败按规矩重试，直到取到或认输。
    pub async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        let mut attempts = 0u32;
        loop {
            let why = {
                let _slot = self.slots.acquire().await;
                // 拿到名额之后再看断没断：排队的这段时间里服务可能已经被认定连不上了。
                if let Some(reason) = self.health().blocked(self.transport.now()) {
                    return Err(FetchError::Unreachable { reason });
                }
                attempts += 1;
                match self.transport.get(url, self.policy.attempt_timeout).await {
                    Ok(bytes) => {
                        self.health().answered();
                        return Ok(bytes);
                    }
                    Err(Failure::NotFound) => {
                        self.health().answered();
                        return Err(FetchError::NotFound);
                    }
                    Err(Failure::Rejected(status)) => {
                        self.health().answered();
                        return Err(FetchError::Rejected(status));
                    }
                    Err(Failure::Busy(status)) => {
                        self.health().answered();
                        format!("HTTP {status}")
                    }
                    Err(Failure::Unreached(why)) => {
                        self.health()
                            .failed(self.transport.now(), &why, &self.policy);
                        why
                    }
                }
            };
            // 名额到这里已经交回去了：等重试的这段时间别的文件照常走。
            let Some(wait) = self.policy.backoff.get(attempts as usize - 1).copied() else {
                return Err(FetchError::Exhausted {
                    attempts,
                    last: why,
                });
            };
            if let Some(reason) = self.health().blocked(self.transport.now()) {
                return Err(FetchError::Unreachable { reason });
            }
            self.transport.sleep(wait).await;
        }
    }

    fn health(&self) -> MutexGuard<'_, Health> {
        self.health
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// 「模型服务还通不通」的账，所有请求共用一份。
#[derive(Debug, Default)]
struct Health {
    /// 上一次得到服务的回答之后，连着有几次请求压根没得到回答。
    failures: u32,
    /// 这一串从什么时候开始。
    failing_since: Option<Duration>,
    /// 断路到什么时候为止，以及认定连不上时的那次失败。
    open: Option<(Duration, String)>,
}

impl Health {
    /// 这一刻是不是断着；断路期满就合上，从头记账。
    fn blocked(&mut self, now: Duration) -> Option<String> {
        let (until, reason) = self.open.as_ref()?;
        if now < *until {
            return Some(reason.clone());
        }
        *self = Self::default();
        None
    }

    /// 服务有了回答——文件、明确的拒绝、或者一个 5xx：这一串到此为止，断着的也合上。
    fn answered(&mut self) {
        *self = Self::default();
    }

    /// 记一次「压根没得到回答」，够条件就断路。
    ///
    /// 「持续了多久」从这一串的**第一次失败**算起，不从请求发出算起：一批下载了好几秒
    /// 的大文件被同一下抖动一起打断时，后一种算法会当场断路。
    fn failed(&mut self, now: Duration, why: &str, policy: &Policy) {
        self.failures += 1;
        let since = *self.failing_since.get_or_insert(now);
        if self.open.is_none()
            && self.failures >= policy.trip_failures
            && now.saturating_sub(since) >= policy.trip_after
        {
            self.open = Some((now + policy.cooldown, why.to_owned()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::task::{Context, Poll, Wake, Waker};

    /// 假的外界：钟是虚拟的，每次请求的结局由剧本说了算。
    ///
    /// 钟只在所有 future 都走不动的时候才往前拨（拨到最近的一个到点时刻），所以
    /// 「同时在途几个」「第几秒发生了什么」都是确定的，与机器快慢无关。
    struct World {
        now: Cell<Duration>,
        /// 等到点的那些：到点时刻，以及到点叫醒谁。
        timers: RefCell<Vec<(Duration, Waker)>>,
        /// 一次请求从发出到有结果要多久。
        latency: Duration,
        in_flight: Cell<usize>,
        peak: Cell<usize>,
        /// 每一次请求：发出的时刻与地址。
        attempts: RefCell<Vec<(Duration, String)>>,
        /// 剧本：在这个时刻有结果的、发往这个地址的请求得到什么。
        script: Box<dyn Fn(Duration, &str) -> Result<Vec<u8>, Failure>>,
    }

    impl World {
        fn new(
            latency: Duration,
            script: impl Fn(Duration, &str) -> Result<Vec<u8>, Failure> + 'static,
        ) -> Self {
            Self {
                now: Cell::new(Duration::ZERO),
                timers: RefCell::new(Vec::new()),
                latency,
                in_flight: Cell::new(0),
                peak: Cell::new(0),
                attempts: RefCell::new(Vec::new()),
                script: Box::new(script),
            }
        }

        fn until(&self, at: Duration) -> Until<'_> {
            Until { world: self, at }
        }

        fn sent(&self) -> usize {
            self.attempts.borrow().len()
        }

        fn sent_to(&self, url: &str) -> Vec<Duration> {
            self.attempts
                .borrow()
                .iter()
                .filter(|(_, to)| to == url)
                .map(|(at, _)| *at)
                .collect()
        }

        /// 把一个 future 跑到底：谁都走不动了，就把钟拨到最近的到点时刻。
        fn run<F: Future>(&self, future: F) -> F::Output {
            let mut future = std::pin::pin!(future);
            let woken = Arc::new(Woken(AtomicBool::new(false)));
            let waker = Waker::from(woken.clone());
            let mut cx = Context::from_waker(&waker);
            loop {
                if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
                    return output;
                }
                if woken.0.swap(false, Ordering::SeqCst) {
                    continue;
                }
                let next = self
                    .timers
                    .borrow()
                    .iter()
                    .map(|(at, _)| *at)
                    .min()
                    .expect("没有人被叫醒，也没有定时器在等：调度卡死了");
                self.now.set(self.now.get().max(next));
                let (due, later): (Vec<_>, Vec<_>) = self
                    .timers
                    .borrow_mut()
                    .drain(..)
                    .partition(|(at, _)| *at <= next);
                *self.timers.borrow_mut() = later;
                for (_, waker) in due {
                    waker.wake();
                }
            }
        }

        /// 一批文件一起取，结果按给的顺序回来。
        fn fetch_all(
            &self,
            fetcher: &Fetcher<&World>,
            urls: &[String],
        ) -> Vec<Result<Vec<u8>, FetchError>> {
            self.run(futures::future::join_all(
                urls.iter().map(|url| fetcher.fetch(url)),
            ))
        }
    }

    struct Woken(AtomicBool);

    impl Wake for Woken {
        fn wake(self: Arc<Self>) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    struct Until<'a> {
        world: &'a World,
        at: Duration,
    }

    impl Future for Until<'_> {
        type Output = ();

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if self.world.now.get() >= self.at {
                return Poll::Ready(());
            }
            self.world
                .timers
                .borrow_mut()
                .push((self.at, cx.waker().clone()));
            Poll::Pending
        }
    }

    impl Transport for &World {
        async fn get(&self, url: &str, _timeout: Duration) -> Result<Vec<u8>, Failure> {
            self.attempts
                .borrow_mut()
                .push((self.now.get(), url.to_owned()));
            self.in_flight.set(self.in_flight.get() + 1);
            self.peak.set(self.peak.get().max(self.in_flight.get()));
            self.until(self.now.get() + self.latency).await;
            self.in_flight.set(self.in_flight.get() - 1);
            (self.script)(self.now.get(), url)
        }

        async fn sleep(&self, wait: Duration) {
            self.until(self.now.get() + wait).await;
        }

        fn now(&self) -> Duration {
            self.now.get()
        }
    }

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    fn urls(prefix: &str, count: usize) -> Vec<String> {
        (0..count).map(|n| format!("{prefix}/{n}.mesh")).collect()
    }

    /// 连接层的失败：浏览器里 `fetch` 被拒绝时就是这一句。
    fn unreached() -> Result<Vec<u8>, Failure> {
        Err(Failure::Unreached("TypeError: Failed to fetch".into()))
    }

    /// 整件事的起因：一个 SITE 的网格不许一次全发出去。
    #[test]
    fn no_more_than_the_allowed_number_are_in_flight() {
        let world = World::new(ms(10), |_, url| Ok(url.as_bytes().to_vec()));
        let fetcher = Fetcher::new(&world, Policy::new(Some(8)));
        let wanted = urls("a", 100);

        let got = world.fetch_all(&fetcher, &wanted);

        assert_eq!(world.peak.get(), 8);
        assert_eq!(world.sent(), 100);
        for (url, bytes) in wanted.iter().zip(got) {
            assert_eq!(bytes.as_deref(), Ok(url.as_bytes()), "各拿各的内容");
        }
        // 13 轮、每轮 10 ms：名额一空出来下一个就接上，没有空转。
        assert_eq!(world.now.get(), ms(130));
    }

    #[test]
    fn the_concurrency_setting_is_kept_within_bounds() {
        assert_eq!(Policy::new(None).concurrency, Policy::DEFAULT_CONCURRENCY);
        assert_eq!(Policy::new(Some(0)).concurrency, 1);
        assert_eq!(Policy::new(Some(32)).concurrency, 32);
        assert_eq!(
            Policy::new(Some(100_000)).concurrency,
            Policy::MAX_CONCURRENCY
        );
    }

    /// 状态码按「再问一次有没有用」分：没有的文件再问也没有，服务一时给不出的值得再问。
    #[test]
    fn statuses_are_sorted_by_whether_asking_again_helps() {
        assert_eq!(Failure::from_status(200), None);
        for status in [403, 404] {
            assert_eq!(Failure::from_status(status), Some(Failure::NotFound));
        }
        for status in [408, 429, 500, 502, 503, 504, 599] {
            assert_eq!(Failure::from_status(status), Some(Failure::Busy(status)));
        }
        // 200 以外的 2xx / 3xx 不是一个网格文件，也不值得再问。
        for status in [204, 206, 301, 400, 401, 410, 499] {
            assert_eq!(
                Failure::from_status(status),
                Some(Failure::Rejected(status))
            );
        }
    }

    /// 出厂参数自己要站得住：重试的总跨度比断路的窗口长（见 `Policy::backoff`）。
    #[test]
    fn the_retry_span_outlasts_the_trip_window() {
        let policy = Policy::new(None);
        let span: Duration = policy.backoff.iter().sum();
        assert!(
            span > policy.trip_after,
            "{span:?} vs {:?}",
            policy.trip_after
        );
    }

    /// 服务说没有的文件，再问还是没有。
    #[test]
    fn a_file_that_is_not_there_is_asked_for_once() {
        let world = World::new(ms(5), |_, url| {
            if url.starts_with("gone") {
                Err(Failure::NotFound)
            } else {
                Err(Failure::Rejected(400))
            }
        });
        let fetcher = Fetcher::new(&world, Policy::new(None));
        let wanted = ["gone/a.mesh".to_owned(), "odd/b.mesh".to_owned()];

        let got = world.fetch_all(&fetcher, &wanted);

        assert_eq!(got[0], Err(FetchError::NotFound));
        assert_eq!(got[1], Err(FetchError::Rejected(400)));
        assert_eq!(world.sent(), 2);
    }

    /// 连接断了一下：按间隔再试，文件照样拿到。
    #[test]
    fn a_dropped_connection_is_retried_until_the_file_arrives() {
        let calls = Cell::new(0);
        let world = World::new(ms(10), move |_, _| {
            calls.set(calls.get() + 1);
            if calls.get() <= 2 {
                unreached()
            } else {
                Ok(vec![7])
            }
        });
        let fetcher = Fetcher::new(&world, Policy::new(None));

        let got = world.fetch_all(&fetcher, &["a.mesh".to_owned()]);

        assert_eq!(got[0], Ok(vec![7]));
        // 0 发出、10 失败；等 400；410 发出、420 失败；等 1200；1620 发出。
        assert_eq!(world.sent_to("a.mesh"), [ms(0), ms(410), ms(1_620)]);
    }

    /// 一个文件一直取不到：重试用完就认输，并说清试了几次、最后一次是什么。
    #[test]
    fn a_file_that_keeps_failing_gives_up_after_the_last_retry() {
        let world = World::new(ms(10), |_, _| Err(Failure::Busy(503)));
        let fetcher = Fetcher::new(&world, Policy::new(None));

        let got = world.fetch_all(&fetcher, &["a.mesh".to_owned()]);

        assert_eq!(
            got[0],
            Err(FetchError::Exhausted {
                attempts: 4,
                last: "HTTP 503".into()
            })
        );
        assert_eq!(world.sent(), 4);
        assert_eq!(
            got[0].as_ref().unwrap_err().to_string(),
            "试了 4 次都没取到，最后一次：HTTP 503"
        );
    }

    /// 等着重试的文件不占名额：几个一直失败的文件不许把后面排队的都拖住，
    /// 也不许因为它们就把服务当成连不上。
    #[test]
    fn files_that_keep_failing_do_not_hold_up_or_cut_off_the_rest() {
        let world = World::new(ms(5), |_, url| {
            if url.starts_with("bad") {
                Err(Failure::Busy(500))
            } else {
                Ok(vec![1])
            }
        });
        let fetcher = Fetcher::new(&world, Policy::new(Some(4)));
        // 坏的排在最前面，一上来就把四个名额占满。
        let mut wanted = urls("bad", 4);
        wanted.extend(urls("good", 40));

        let got = world.fetch_all(&fetcher, &wanted);

        assert!(
            got[..4]
                .iter()
                .all(|one| matches!(one, Err(FetchError::Exhausted { attempts: 4, .. })))
        );
        assert!(got[4..].iter().all(|one| one == &Ok(vec![1])));
        assert_eq!(world.peak.get(), 4);
        // 好的那 40 个在第一轮重试的间隔（400 ms）之内就全发出去了。
        let last_good = world
            .attempts
            .borrow()
            .iter()
            .filter(|(_, url)| url.starts_with("good"))
            .map(|(at, _)| *at)
            .max()
            .unwrap();
        assert!(last_good < ms(400), "{last_good:?}");
        // 坏文件最后独自重试了四秒多，紧跟着来的请求照样发得出去。
        let next = world.fetch_all(&fetcher, &urls("good", 1));
        assert_eq!(next[0], Ok(vec![1]));
    }

    /// 服务答得出 5xx 就不是「连不上」：不断路，每个文件把重试走完——这种失败回来得快，
    /// 重试又不占名额，整批也就拖一个重试周期。
    #[test]
    fn a_server_that_answers_with_errors_costs_one_retry_span() {
        let world = World::new(ms(5), |_, _| Err(Failure::Busy(502)));
        let fetcher = Fetcher::new(&world, Policy::new(Some(16)));

        let got = world.fetch_all(&fetcher, &urls("a", 2_000));

        assert!(
            got.iter()
                .all(|one| matches!(one, Err(FetchError::Exhausted { attempts: 4, .. })))
        );
        assert_eq!(world.sent(), 8_000);
        assert!(world.now.get() <= ms(8_000), "{:?}", world.now.get());
    }

    /// 服务断了 3.5 秒（比如重启了一下）：靠重试熬过去，一个文件都不丢。
    #[test]
    fn an_outage_shorter_than_the_trip_window_loses_nothing() {
        let world = World::new(ms(5), |now, _| {
            if (ms(50)..ms(3_550)).contains(&now) {
                unreached()
            } else {
                Ok(vec![1])
            }
        });
        let fetcher = Fetcher::new(&world, Policy::new(Some(8)));

        let got = world.fetch_all(&fetcher, &urls("a", 200));

        assert!(got.iter().all(|one| one == &Ok(vec![1])), "{got:?}");
        assert!(world.sent() > 200, "中断期间确实有请求失败过、重试过");
    }

    /// 服务停了：几秒之内认定连不上，排队的直接判失败——不是每个文件都去撞一遍。
    ///
    /// 失败来得慢（这里设每次要等 2 秒才知道连不上）是最拖时间的情形：没有断路的话
    /// 2000 个文件光第一轮就要 250 秒。
    #[test]
    fn a_long_outage_fails_the_queue_within_seconds() {
        let world = World::new(ms(2_000), |_, _| unreached());
        let fetcher = Fetcher::new(&world, Policy::new(Some(16)));

        let got = world.fetch_all(&fetcher, &urls("a", 2_000));

        assert!(
            got.iter()
                .all(|one| matches!(one, Err(FetchError::Unreachable { .. })))
        );
        assert!(world.now.get() <= ms(10_000), "{:?}", world.now.get());
        assert!(world.sent() <= 100, "发出去的请求：{}", world.sent());
        assert_eq!(
            got[1_999].as_ref().unwrap_err().to_string(),
            "模型服务连不上（TypeError: Failed to fetch），这个文件没有再去取"
        );
    }

    /// 同上，但连接被拒绝得快（几毫秒）：请求会多发一些，时间照样是几秒。
    #[test]
    fn a_long_outage_with_fast_refusals_is_also_over_within_seconds() {
        let world = World::new(ms(5), |_, _| unreached());
        let fetcher = Fetcher::new(&world, Policy::new(Some(16)));

        let got = world.fetch_all(&fetcher, &urls("a", 2_000));

        assert!(got.iter().all(Result::is_err));
        assert!(world.now.get() <= ms(10_000), "{:?}", world.now.get());
        // 每个文件至多走完前三次：第四次要等到 4.6 秒之后，那时头一批一失败就断路了。
        assert!(world.sent() <= 2_000 * 3 + 16, "{}", world.sent());
    }

    /// 断路只管一阵：冷却过后请求照常发，服务回来了就拿得到。
    #[test]
    fn requests_go_out_again_after_the_cooldown() {
        let world = World::new(ms(2_000), |now, _| {
            if now < Duration::from_secs(30) {
                unreached()
            } else {
                Ok(vec![1])
            }
        });
        let fetcher = Fetcher::new(&world, Policy::new(Some(16)));
        let first = world.fetch_all(&fetcher, &urls("a", 200));
        assert!(first.iter().all(Result::is_err));
        let tripped_at = world.now.get();

        // 冷却没过：轮到的文件当场判失败，一个请求都不发。
        let sent = world.sent();
        let during = world.fetch_all(&fetcher, &urls("b", 50));
        assert!(
            during
                .iter()
                .all(|one| matches!(one, Err(FetchError::Unreachable { .. })))
        );
        assert_eq!(world.sent(), sent);
        assert_eq!(world.now.get(), tripped_at, "当场就有结论，不用等");

        // 用户过一会儿再点一次「显示」：服务已经回来了。
        world.now.set(Duration::from_secs(60));
        let later = world.fetch_all(&fetcher, &urls("a", 200));
        assert!(later.iter().all(|one| one == &Ok(vec![1])));
    }

    /// 同一瞬间倒下一大批（一次抖动打断了所有在途连接）不算「持续失败」。
    #[test]
    fn a_burst_of_failures_at_one_instant_does_not_trip() {
        let policy = Policy::new(None);
        let mut health = Health::default();
        for _ in 0..500 {
            health.failed(ms(1_000), "x", &policy);
        }
        assert_eq!(health.blocked(ms(1_000)), None);
    }

    /// 一两个卡住的请求隔很久各失败一次，也不算。
    #[test]
    fn a_handful_of_slow_failures_does_not_trip() {
        let policy = Policy::new(None);
        let mut health = Health::default();
        for n in 0..policy.trip_failures - 1 {
            health.failed(Duration::from_secs(60 * u64::from(n)), "超时", &policy);
        }
        assert_eq!(health.blocked(Duration::from_secs(600)), None);
    }

    /// 每半秒失败一次：第 9 次（4.0 秒）攒够了次数也攒够了时间，断路，冷却到 9.0 秒。
    fn tripped(policy: &Policy) -> Health {
        let mut health = Health::default();
        for n in 0..10 {
            health.failed(ms(500 * n), "x", policy);
        }
        health
    }

    /// 断着的时候有请求得到了回答（断路之前发出去的）：服务明明通着，当场合上。
    #[test]
    fn an_answer_closes_the_breaker() {
        let policy = Policy::new(None);
        let mut health = tripped(&policy);
        assert_eq!(health.blocked(ms(5_000)).as_deref(), Some("x"));
        health.answered();
        assert_eq!(health.blocked(ms(5_000)), None);
    }

    /// 断路期满自己合上，而且从头记账：下一串失败要重新攒够时间和次数。
    #[test]
    fn the_breaker_closes_by_itself_and_starts_counting_afresh() {
        let policy = Policy::new(None);
        let mut health = tripped(&policy);
        assert!(health.blocked(ms(8_999)).is_some());
        assert_eq!(health.blocked(ms(9_000)), None);
        health.failed(ms(9_001), "y", &policy);
        assert_eq!(health.blocked(ms(9_002)), None);
    }
}
