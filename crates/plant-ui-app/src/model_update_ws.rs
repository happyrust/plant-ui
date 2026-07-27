//! 手动更新执行进度的跨平台 WebSocket。

use std::sync::mpsc::Sender;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

#[cfg(not(target_arch = "wasm32"))]
use ewebsock::{Options, WsEvent, WsMessage, WsReceiver, WsSender};
use plant_ui::model_update::ProgressEvent;

use crate::data::Evt;

#[cfg(not(target_arch = "wasm32"))]
const PING_EVERY: Duration = Duration::from_secs(30);
#[cfg(not(target_arch = "wasm32"))]
const RECONNECT_DELAY: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Run(String),
    Queue,
}

impl Scope {
    fn wants(&self, task_id: &str) -> bool {
        match self {
            Self::Run(mine) => mine == task_id,
            Self::Queue => true,
        }
    }

    fn progress(&self, task_id: String, event: ProgressEvent) -> Evt {
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

#[cfg(not(target_arch = "wasm32"))]
pub struct Feed {
    url: String,
    scope: Scope,
    tx: Sender<Evt>,
    ctx: egui::Context,
    socket: Option<(WsSender, WsReceiver)>,
    reconnect_at: Instant,
    last_ping: Instant,
}

#[cfg(not(target_arch = "wasm32"))]
impl Feed {
    pub fn open(base: &str, scope: Scope, tx: Sender<Evt>, ctx: egui::Context) -> Self {
        let mut feed = Self {
            url: ws_url(base),
            scope,
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
                        let _ = self.tx.send(self.scope.live());
                    }
                    WsEvent::Message(WsMessage::Text(text)) => {
                        if let Some((task_id, event)) = decode(&text, &self.scope) {
                            let _ = self.tx.send(self.scope.progress(task_id, event));
                        }
                        if self.scope == Scope::Queue && starts_or_finishes(&text) {
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
            let _ = self.tx.send(self.scope.down(reason));
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
                let _ = self.tx.send(self.scope.down(error));
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
    pub fn open(_base: &str, scope: Scope, tx: Sender<Evt>, ctx: egui::Context) -> Self {
        // ponytail: 浏览器端先用已有轮询收口；需要逐单元实时明细时再保留 wasm socket。
        let _ = tx.send(scope.down("浏览器端使用轮询获取任务进度".into()));
        ctx.request_repaint();
        Self
    }

    pub fn poll(&mut self) {}
}

fn ws_url(base: &str) -> String {
    let base = base.trim_end_matches('/');
    let swapped = match base.split_once("://") {
        Some(("https", rest)) => format!("wss://{rest}"),
        Some(("http", rest)) => format!("ws://{rest}"),
        _ => base.to_owned(),
    };
    format!("{swapped}/api/v1/ws")
}

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
        assert!(decode(unit, &Scope::Run("db-1".into())).is_some());
        assert!(decode(unit, &Scope::Run("db-2".into())).is_none());
        assert!(starts_or_finishes(r#"{"type":"task_finished"}"#));
    }
}
