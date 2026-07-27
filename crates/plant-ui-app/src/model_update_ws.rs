//! 手动更新执行进度的长连接。本仓库第一条 WebSocket（ADR-0005）。
//!
//! 逐单元事件只在 `/api/v1/ws` 的 tasks 主题上发，`GET /tasks/{id}` 只给得出 `state`、
//! `events_seen` 与终态 `result`。所以轮询留着兜终态，明细走这里，两条通道并存。
//!
//! 形态跟着 `data.rs` 走：eframe 是同步绘制，长连接开一根常驻线程，收到的事件经同一条
//! `Evt` channel 回 UI。tungstenite 是阻塞式的，正好配这个线程模型。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use plant_ui::model_update::ProgressEvent;
use tungstenite::Message;

use crate::data::Evt;

/// 服务端 90s 无入站消息就断，约定客户端 30s 一次 ping（spec §5.4）。
const PING_EVERY: Duration = Duration::from_secs(30);
/// 读超时决定了停止标志多久被看一眼，也决定了 ping 的抖动上限。
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const RECONNECT_DELAY: Duration = Duration::from_secs(3);

/// 这条连接是为谁开的。两种订阅者共用同一段连接与重连逻辑，只在「事件往哪送」
/// 上分叉——tasks 主题上跑的是同一批消息，没有理由维护两套收发。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// 只认这一个运行实例，别人的事件一概不收（模型更新向导第三步）。
    Run(String),
    /// 全部任务，逐条带着 task_id 送回去（任务队列视图的行内明细）。
    Queue,
}

impl Scope {
    fn wants(&self, task_id: &str) -> bool {
        match self {
            Self::Run(mine) => mine == task_id,
            Self::Queue => true,
        }
    }

    fn progress(&self, task_id: String, event: plant_ui::model_update::ProgressEvent) -> Evt {
        match self {
            Self::Run(_) => Evt::ModelUpdateProgress(event),
            Self::Queue => Evt::QueueProgress(task_id, event),
        }
    }

    fn live(&self) -> Evt {
        match self {
            Self::Run(_) => Evt::ModelUpdateFeedLive,
            Self::Queue => Evt::QueueFeedLive,
        }
    }

    fn down(&self, reason: String) -> Evt {
        match self {
            Self::Run(_) => Evt::ModelUpdateFeedDown(reason),
            Self::Queue => Evt::QueueFeedDown(reason),
        }
    }
}

/// 一条活着的明细连接。丢掉它就等于停。
pub struct Feed {
    stop: Arc<AtomicBool>,
}

impl Feed {
    /// 起一条订阅 tasks 主题的连接。断开后自动重连，直到被丢弃——
    /// 任务还没跑完就不该放弃明细。
    pub fn open(base: &str, scope: Scope, tx: Sender<Evt>, ctx: egui::Context) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let url = ws_url(base);
        let thread_stop = stop.clone();
        let thread_tx = tx.clone();
        let thread_ctx = ctx.clone();
        let thread_scope = scope.clone();
        let spawned = std::thread::Builder::new()
            .name("plant-ui-model-ws".into())
            .spawn(move || run(&url, &thread_scope, &thread_tx, &thread_ctx, &thread_stop));
        if let Err(error) = spawned {
            // 线程都起不来时至少让界面说得出话，别停在「连接中」。
            let _ = tx.send(scope.down(format!("无法启动连接线程：{error}")));
            ctx.request_repaint();
        }
        Self { stop }
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// `http://host:port` → `ws://host:port/api/v1/ws`。地址来自设置项，可能带 https。
fn ws_url(base: &str) -> String {
    let base = base.trim_end_matches('/');
    let swapped = match base.split_once("://") {
        Some(("https", rest)) => format!("wss://{rest}"),
        Some(("http", rest)) => format!("ws://{rest}"),
        // 已经是 ws / wss，或者压根没写协议：原样交给 tungstenite 去判。
        _ => base.to_owned(),
    };
    format!("{swapped}/api/v1/ws")
}

fn run(url: &str, scope: &Scope, tx: &Sender<Evt>, ctx: &egui::Context, stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        match session(url, scope, tx, ctx, stop) {
            Ok(()) => {}
            Err(error) => {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let _ = tx.send(scope.down(error));
                ctx.request_repaint();
            }
        }
        // 退出 session 一定是断了（正常收尾也走这里），歇一会儿再连。
        let deadline = Instant::now() + RECONNECT_DELAY;
        while Instant::now() < deadline {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

/// 一次连接的完整生命周期。返回即代表这条连接没了，由调用方决定是否重连。
fn session(
    url: &str,
    scope: &Scope,
    tx: &Sender<Evt>,
    ctx: &egui::Context,
    stop: &AtomicBool,
) -> Result<(), String> {
    let (mut socket, _) =
        tungstenite::connect(url).map_err(|e| format!("连接 {url} 失败：{e}"))?;
    set_read_timeout(&socket).map_err(|e| format!("设置读超时失败：{e}"))?;

    socket
        .send(Message::Text(
            r#"{"type":"subscribe","topics":["tasks"]}"#.into(),
        ))
        .map_err(|e| format!("订阅 tasks 主题失败：{e}"))?;

    let _ = tx.send(scope.live());
    ctx.request_repaint();

    let mut last_ping = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = socket.close(None);
            return Ok(());
        }
        if last_ping.elapsed() >= PING_EVERY {
            socket
                .send(Message::Text(r#"{"type":"ping"}"#.into()))
                .map_err(|e| format!("心跳发送失败：{e}"))?;
            last_ping = Instant::now();
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                if let Some((task_id, event)) = decode(&text, scope) {
                    let _ = tx.send(scope.progress(task_id, event));
                    ctx.request_repaint();
                }
                // 队列视图还要靠起讫信封把轮询叫醒：终态由轮询收口，但等下一拍
                // 才刷新会让刚跑完的那一行多停一秒在「生成中」上。
                if *scope == Scope::Queue && starts_or_finishes(&text) {
                    let _ = tx.send(Evt::QueueTaskChanged);
                    ctx.request_repaint();
                }
            }
            Ok(Message::Close(_)) => return Err("服务端关闭了连接".into()),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                // 读超时只是没消息，不是断线：回去看一眼停止标志和心跳。
            }
            Err(e) => return Err(format!("连接中断：{e}")),
        }
    }
}

/// tungstenite 把流包在 `MaybeTlsStream` 里，超时要设在底下那个 TcpStream 上。
/// 没有超时的话 `read()` 会一直阻塞，停止标志得等下一条消息才看得见——而
/// 「任务跑完之后不再有消息」恰好是最常见的收尾情形。
fn set_read_timeout(
    socket: &tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
) -> std::io::Result<()> {
    match socket.get_ref() {
        tungstenite::stream::MaybeTlsStream::Plain(tcp) => tcp.set_read_timeout(Some(READ_TIMEOUT)),
        // 服务端不上 TLS（spec §1 非目标）。真走到这里就退化成阻塞读，功能不受影响。
        _ => Ok(()),
    }
}

/// 信封是 `{ type, seq, ts, task_id, payload }`。只解 `task_progress`，
/// 终态一律靠轮询收口（断线期间的 `task_finished` 会丢），这里不拿它改状态。
fn decode(text: &str, scope: &Scope) -> Option<(String, ProgressEvent)> {
    let envelope: serde_json::Value = serde_json::from_str(text).ok()?;
    if envelope.get("type")?.as_str()? != "task_progress" {
        return None;
    }
    let task_id = envelope.get("task_id")?.as_str()?;
    if !scope.wants(task_id) {
        return None;
    }
    let event = serde_json::from_value(envelope.get("payload")?.clone()).ok()?;
    Some((task_id.to_owned(), event))
}

/// 队列视图用的醒钟：只看信封类型，**不拿它当状态来源**。
fn starts_or_finishes(text: &str) -> bool {
    let Ok(envelope) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    matches!(
        envelope.get("type").and_then(|v| v.as_str()),
        Some("task_started" | "task_finished")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_url_swaps_scheme_and_appends_the_endpoint() {
        assert_eq!(
            ws_url("http://127.0.0.1:8021"),
            "ws://127.0.0.1:8021/api/v1/ws"
        );
        assert_eq!(ws_url("https://host/"), "wss://host/api/v1/ws");
        assert_eq!(ws_url("ws://host"), "ws://host/api/v1/ws");
    }

    #[test]
    fn decode_takes_only_this_runs_progress() {
        let unit = r#"{"type":"task_progress","seq":3,"task_id":"db-7997-1","payload":
            {"kind":"model_unit_started","dbnum":7997,"root_refno":"24381/100817","noun":"BRAN"}}"#;
        let mine = Scope::Run("db-7997-1".into());
        assert!(matches!(
            decode(unit, &mine),
            Some((_, ProgressEvent::ModelUnitStarted { .. }))
        ));
        // 别人的任务、别的信封类型都不该混进本次运行的行列表。
        assert!(decode(unit, &Scope::Run("db-8000-2".into())).is_none());
        assert!(decode(r#"{"type":"pong","task_id":"db-7997-1","payload":{}}"#, &mine).is_none());
    }

    /// 队列视图收全部任务的明细，并且要知道**这一条是谁的**——一条连接铺很多行，
    /// 分不清 task_id 就会把 db8000 的单元画进 db7997 那一行。
    #[test]
    fn queue_scope_keeps_every_task_and_tags_it() {
        let unit = r#"{"type":"task_progress","task_id":"db-8000-2","payload":
            {"kind":"model_unit_started","dbnum":8000,"root_refno":"24381/7","noun":"EQUI"}}"#;
        let (task_id, _) = decode(unit, &Scope::Queue).expect("队列范围收所有任务");
        assert_eq!(task_id, "db-8000-2");
        // 起讫信封只当醒钟：叫轮询早一拍去取，不拿它改行状态。
        assert!(starts_or_finishes(
            r#"{"type":"task_finished","task_id":"db-8000-2"}"#
        ));
        assert!(!starts_or_finishes(unit));
    }
}
