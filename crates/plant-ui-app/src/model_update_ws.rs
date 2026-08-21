//! 任务队列明细的跨平台 WebSocket：订阅全部任务的 `task_progress`，逐条带
//! task_id 交给队列视图分桶；起讫信封（`task_started` / `task_finished`）只当
//! 醒钟用，叫轮询早一拍去取快照。
//!
//! 它曾经还有一种「跟单次运行」的订阅形态（`Scope::Run`），随向导第三步一起
//! 退役（ADR-0011）：执行进度只在队列视图，一条连接订全部。

use std::sync::mpsc::Sender;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

#[cfg(not(target_arch = "wasm32"))]
use ewebsock::{Options, WsEvent, WsMessage, WsReceiver, WsSender};
#[cfg(not(target_arch = "wasm32"))]
use plant_ui::model_update::ProgressEvent;

use crate::data::Evt;

#[cfg(not(target_arch = "wasm32"))]
const PING_EVERY: Duration = Duration::from_secs(30);
#[cfg(not(target_arch = "wasm32"))]
const RECONNECT_DELAY: Duration = Duration::from_secs(3);

#[cfg(not(target_arch = "wasm32"))]
pub struct Feed {
    url: String,
    tx: Sender<Evt>,
    ctx: egui::Context,
    socket: Option<(WsSender, WsReceiver)>,
    reconnect_at: Instant,
    last_ping: Instant,
}

#[cfg(not(target_arch = "wasm32"))]
impl Feed {
    pub fn open(base: &str, tx: Sender<Evt>, ctx: egui::Context) -> Self {
        let mut feed = Self {
            url: ws_url(base),
            tx,
            ctx,
            socket: None,
            reconnect_at: Instant::now(),
            last_ping: Instant::now(),
        };
        feed.reconnect();
        feed
    }

    pub fn poll(&mut self) {
        if self.socket.is_none() && Instant::now() >= self.reconnect_at {
            self.reconnect();
        }

        let mut disconnected = None;
        if let Some((sender, receiver)) = &mut self.socket {
            while let Some(event) = receiver.try_recv() {
                match event {
                    WsEvent::Opened => {
                        sender.send(WsMessage::Text(
                            r#"{"type":"subscribe","topics":["tasks"]}"#.into(),
                        ));
                        let _ = self.tx.send(Evt::QueueFeedLive);
                    }
                    WsEvent::Message(WsMessage::Text(text)) => {
                        if let Some((task_id, event)) = decode(&text) {
                            let _ = self.tx.send(Evt::QueueProgress(task_id, event));
                        }
                        if starts_or_finishes(&text) {
                            let _ = self.tx.send(Evt::QueueTaskChanged);
                        }
                    }
                    WsEvent::Error(error) => disconnected = Some(error),
                    WsEvent::Closed => disconnected = Some("服务端关闭了连接".into()),
                    _ => {}
                }
            }
            if self.last_ping.elapsed() >= PING_EVERY {
                sender.send(WsMessage::Text(r#"{"type":"ping"}"#.into()));
                self.last_ping = Instant::now();
            }
        }

        if let Some(reason) = disconnected {
            self.socket = None;
            self.reconnect_at = Instant::now() + RECONNECT_DELAY;
            let _ = self.tx.send(Evt::QueueFeedDown(reason));
        }
        self.ctx.request_repaint_after(Duration::from_secs(1));
    }

    fn reconnect(&mut self) {
        match ewebsock::connect_with_wakeup(self.url.clone(), Options::default(), {
            let ctx = self.ctx.clone();
            move || ctx.request_repaint()
        }) {
            Ok(socket) => self.socket = Some(socket),
            Err(error) => {
                let _ = self.tx.send(Evt::QueueFeedDown(error));
                self.reconnect_at = Instant::now() + RECONNECT_DELAY;
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Feed {
    fn drop(&mut self) {
        if let Some((sender, _)) = &mut self.socket {
            sender.close();
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub struct Feed;

#[cfg(target_arch = "wasm32")]
impl Feed {
    pub fn open(_base: &str, tx: Sender<Evt>, ctx: egui::Context) -> Self {
        // 浏览器端先用已有轮询收口；需要逐单元实时明细时再保留 wasm socket。
        //
        // 发的是「没订阅」而**不是**「断线」：这个构建从来没连过，说断线是在指控
        // 一次不存在的故障，而随之而来的「明细缺 N 条」会等于服务端发过的全部。
        let _ = tx.send(Evt::QueueFeedUnsubscribed(
            "浏览器端进度走轮询，不建实时连接".into(),
        ));
        ctx.request_repaint();
        Self
    }

    pub fn poll(&mut self) {}
}

#[cfg(not(target_arch = "wasm32"))]
fn ws_url(base: &str) -> String {
    let base = base.trim_end_matches('/');
    let swapped = match base.split_once("://") {
        Some(("https", rest)) => format!("wss://{rest}"),
        Some(("http", rest)) => format!("ws://{rest}"),
        _ => base.to_owned(),
    };
    format!("{swapped}/api/v1/ws")
}

#[cfg(not(target_arch = "wasm32"))]
fn decode(text: &str) -> Option<(String, ProgressEvent)> {
    let envelope: serde_json::Value = serde_json::from_str(text).ok()?;
    if envelope.get("type")?.as_str()? != "task_progress" {
        return None;
    }
    let task_id = envelope.get("task_id")?.as_str()?;
    let event = serde_json::from_value(envelope.get("payload")?.clone()).ok()?;
    Some((task_id.to_owned(), event))
}

#[cfg(not(target_arch = "wasm32"))]
fn starts_or_finishes(text: &str) -> bool {
    let Ok(envelope) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    matches!(
        envelope.get("type").and_then(|value| value.as_str()),
        Some("task_started" | "task_finished")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_websocket_events() {
        assert_eq!(ws_url("https://host/"), "wss://host/api/v1/ws");
        let unit = r#"{"type":"task_progress","task_id":"db-1","payload":
            {"kind":"model_unit_started","dbnum":1,"root_refno":"1/2","noun":"BRAN"}}"#;
        let (task_id, _) = decode(unit).expect("契约 payload 应当能解出来");
        assert_eq!(task_id, "db-1");
        assert!(decode(r#"{"type":"task_started","task_id":"db-1"}"#).is_none());
        assert!(starts_or_finishes(r#"{"type":"task_finished"}"#));
    }
}
