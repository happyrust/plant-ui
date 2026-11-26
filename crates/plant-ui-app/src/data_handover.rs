//! 新入口「三维数据提资」的宿主一侧：请求、回包，以及 `App` 上接这扇窗的两段。
//!
//! 绘制与判据在 `plant_ui::data_handover` / `plant_ui::data_publish_check`，这里只做往返：
//!
//! - 提资前检查：逐个元素问模型服务（`GET /api/v1/node/model-status`）。只读，不排任何任务。
//! - 「更新到最新」：两次执行请求（`POST /api/v1/update/execute`），先发落后的库、回执到了再发需初始化的库
//!   ——队列先进先出，增量更新就排在首次导入前面。
//! - 预览 / 查待删除 / 提交：与旧入口「三维数据接口」打同一组 rs-server 接口。
//!
//! 旧入口的命令与回包一行不动：这里另起一组请求与回包，两边各落各的状态。

use futures::StreamExt;
use plant_ui::RefU64;
use plant_ui::data_handover::{self as window, Action};
use plant_ui::data_publish::{DeletionCandidate, PublishPreviewData, PublishRequest};
use plant_ui::data_publish_check::{
    self as check, ElementCheck, ElementFact, NodeModelStatus, Outcome,
};
use plant_ui::model_update::{Enqueued, Failure};

use crate::data_publish_api::{self, SubmitResult};
use crate::{App, data, logs, model_update_api};

/// 同时在途的检查请求数。一次往返二三十毫秒：元素多时不至于排成一长串，也不把服务端打满。
const CHECK_CONCURRENCY: usize = 8;

pub enum Req {
    Check {
        epoch: u64,
        base: String,
        project: String,
        mdb: String,
        namespace: String,
        refnos: Vec<RefU64>,
    },
    Update {
        base: String,
        project: String,
        mdb: String,
        namespace: String,
        lagging: Vec<u32>,
        needs_init: Vec<u32>,
    },
    Preview {
        base: String,
        request: PublishRequest,
    },
    LookupDeletions {
        base: String,
        request: PublishRequest,
    },
    Publish {
        base: String,
        request: PublishRequest,
    },
}

/// 一份执行请求：问了哪些库、回了什么。回执个头大，装箱之后 [`Evt`] 的各变体才不至于大小悬殊。
pub type Executed = Box<(Vec<u32>, anyhow::Result<Enqueued>)>;

pub enum Evt {
    Check {
        epoch: u64,
        outcome: Outcome,
    },
    /// 「更新到最新」的两份回执。`None` = 那一份没有发（名单是空的，或前一份没发成）。
    Update {
        lagging: Option<Executed>,
        needs_init: Option<Executed>,
    },
    Preview(anyhow::Result<PublishPreviewData>),
    Deletions(anyhow::Result<Vec<DeletionCandidate>>),
    Published(PublishRequest, anyhow::Result<SubmitResult>),
}

pub async fn handle(req: Req) -> Evt {
    match req {
        Req::Check {
            epoch,
            base,
            project,
            mdb,
            namespace,
            refnos,
        } => {
            let facts = futures::stream::iter(refnos)
                .map(|refno| {
                    let (base, project, mdb, namespace) = (&base, &project, &mdb, &namespace);
                    async move {
                        let result = model_update_api::node_model_status(
                            base,
                            &refno.to_string(),
                            project,
                            mdb,
                            namespace,
                        )
                        .await;
                        (refno, result)
                    }
                })
                .buffered(CHECK_CONCURRENCY)
                .collect::<Vec<_>>()
                .await;
            Evt::Check {
                epoch,
                outcome: outcome_of(facts, clock()),
            }
        }
        Req::Update {
            base,
            project,
            mdb,
            namespace,
            lagging,
            needs_init,
        } => {
            let first = if lagging.is_empty() {
                None
            } else {
                Some(
                    model_update_api::execute(&base, &project, &mdb, &namespace, Some(&lagging))
                        .await,
                )
            };
            // 顺序靠「回执到了再发」保证。前一份没发成就不发后一份：顺序已经保不住了，由人再点一次。
            let second = if needs_init.is_empty() || matches!(first, Some(Err(_))) {
                None
            } else {
                Some(
                    model_update_api::execute(&base, &project, &mdb, &namespace, Some(&needs_init))
                        .await,
                )
            };
            Evt::Update {
                lagging: first.map(|result| Box::new((lagging, result))),
                needs_init: second.map(|result| Box::new((needs_init, result))),
            }
        }
        Req::Preview { base, request } => {
            Evt::Preview(data_publish_api::preview(&base, &request).await)
        }
        Req::LookupDeletions { base, request } => {
            Evt::Deletions(data_publish_api::lookup_deletions(&base, &request).await)
        }
        Req::Publish { base, request } => {
            let result = data_publish_api::submit(&base, &request).await;
            Evt::Published(request, result)
        }
    }
}

/// 「检查于 HH:MM:SS」的那个钟点。
pub fn clock() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

/// 把逐个元素的往返结果收成一次检查的结果。
///
/// 失败按服务端给的码分三种，不解析 message：没有这条路由（老服务端）→ 整次检查不适用，提资照旧；
/// 文件里没有这个元素 → 这个元素已删除；其余（连不上、超时、内部错）→ 没查成，既不当最新也不当落后。
fn outcome_of(facts: Vec<(RefU64, anyhow::Result<NodeModelStatus>)>, at: String) -> Outcome {
    let mut elements = Vec::with_capacity(facts.len());
    for (refno, result) in facts {
        let fact = match result {
            Ok(status) => ElementFact::Checked(Box::new(status)),
            Err(error) => {
                let failure = model_update_api::failure_of(&error);
                match failure.code.as_str() {
                    model_update_api::ROUTE_MISSING => {
                        return Outcome::Unsupported(check::UNSUPPORTED_NO_ROUTE.to_owned());
                    }
                    "REFNO_NOT_FOUND" => ElementFact::Gone,
                    _ => ElementFact::Failed(failed_reason(&failure)),
                }
            }
        };
        elements.push(ElementCheck { refno, fact });
    }
    check::settle(elements, at)
}

/// 没查成的那一句。连不上与超时在请求层归成同一个码（`timeout`），原文是传输层的一句英文（`io: Connection refused`），
/// 前面垫一句人话，原文留在括号里；服务端自己说出来的原因（库文件打不开、水位读不到……）原话照登。
fn failed_reason(failure: &Failure) -> String {
    match failure.code.as_str() {
        "timeout" => format!("连不上模型服务，或请求超时（{}）", failure.message),
        _ => failure.message.clone(),
    }
}

/// 请求根本没发出去时给界面的回音：每个元素都算没查成，不让它一直转圈。
fn unsent(refnos: Vec<RefU64>, reason: &str) -> Outcome {
    Outcome::Checked {
        elements: refnos
            .into_iter()
            .map(|refno| ElementCheck {
                refno,
                fact: ElementFact::Failed(reason.to_owned()),
            })
            .collect(),
        at: clock(),
    }
}

const WORKER_DOWN: &str = "数据请求线程不可用";

impl App {
    /// `Cmd::DataHandover` 的处置。
    pub(crate) fn apply_data_handover(&mut self, action: Action) {
        match action {
            Action::Open => {
                // 两扇窗共用预览与确认删除两扇子窗，同时开着会抢同一个子窗：后开的顶掉先开的。
                // 旧窗一次提交在途时不顶——回包要落回它身上。
                if self.data_publish_state.open && self.data_publish_state.submitting {
                    self.logs.warn(
                        &mut self.vm.logs,
                        "「三维数据接口」正在提交，等它回来再打开「三维数据提资」",
                    );
                    return;
                }
                self.data_publish_state.open = false;
                self.data_handover_state.request_open();
            }
            Action::Check { epoch, refnos } => {
                let sent = self
                    .bridge
                    .req
                    .send(data::Req::DataHandover(Req::Check {
                        epoch,
                        base: self.model_api_url.clone(),
                        project: self.vm.project.clone(),
                        mdb: self.mdb.clone(),
                        namespace: self.namespace.clone(),
                        refnos: refnos.clone(),
                    }))
                    .is_ok();
                if !sent {
                    self.data_handover_state
                        .adopt_check(epoch, unsent(refnos, WORKER_DOWN));
                }
            }
            Action::Update {
                lagging,
                needs_init,
            } => {
                // 按钮灰不灰由检查结果定；这里是最后一道（身份没对上、服务不消化执行请求），理由已进日志。
                if !self.model_service_executable() {
                    self.data_handover_state
                        .update_failed("模型服务此刻不接受执行请求，原因见日志");
                    return;
                }
                self.logs.info(
                    &mut self.vm.logs,
                    format!(
                        "提资前更新：更新请求已提交（增量更新 {} 个库、首次导入 {} 个库），任务排上后进度在任务队列里",
                        lagging.len(),
                        needs_init.len()
                    ),
                );
                let sent = self
                    .bridge
                    .req
                    .send(data::Req::DataHandover(Req::Update {
                        base: self.model_api_url.clone(),
                        project: self.vm.project.clone(),
                        mdb: self.mdb.clone(),
                        namespace: self.namespace.clone(),
                        lagging,
                        needs_init,
                    }))
                    .is_ok();
                if !sent {
                    self.data_handover_state.update_failed(WORKER_DOWN);
                }
            }
            Action::Preview(request) => {
                let sent = self
                    .bridge
                    .req
                    .send(data::Req::DataHandover(Req::Preview {
                        base: self.data_api_url.clone(),
                        request,
                    }))
                    .is_ok();
                if !sent {
                    self.data_handover_state
                        .adopt_preview(Err(WORKER_DOWN.to_owned()));
                }
            }
            Action::LookupDeletions(request) => {
                let sent = self
                    .bridge
                    .req
                    .send(data::Req::DataHandover(Req::LookupDeletions {
                        base: self.data_api_url.clone(),
                        request,
                    }))
                    .is_ok();
                if !sent {
                    self.data_handover_state
                        .adopt_deletions(Err(WORKER_DOWN.to_owned()));
                }
            }
            Action::Publish(request) => {
                let sent = self
                    .bridge
                    .req
                    .send(data::Req::DataHandover(Req::Publish {
                        base: self.data_api_url.clone(),
                        request,
                    }))
                    .is_ok();
                if !sent {
                    self.data_handover_state
                        .adopt_published(Err(WORKER_DOWN.to_owned()));
                }
            }
        }
    }

    /// `Evt::DataHandover` 的处置。
    pub(crate) fn settle_data_handover(&mut self, ctx: &egui::Context, evt: Evt) {
        match evt {
            Evt::Check { epoch, outcome } => self.data_handover_state.adopt_check(epoch, outcome),
            Evt::Update {
                lagging,
                needs_init,
            } => {
                let mut failed = None;
                for (kind, executed) in [("增量更新", lagging), ("首次导入", needs_init)] {
                    let Some(executed) = executed else {
                        continue;
                    };
                    let (asked, result) = *executed;
                    match result {
                        Ok(receipt) => {
                            self.logs.info(
                                &mut self.vm.logs,
                                window::receipt_line(kind, &asked, &receipt),
                            );
                            for warning in &receipt.warnings {
                                self.logs
                                    .warn(&mut self.vm.logs, format!("入队告警：{warning}"));
                            }
                            self.data_handover_state.update_receipt(&receipt);
                        }
                        Err(error) => {
                            failed =
                                Some(format!("{kind}没有排上（{}）", logs::error_chain(&error)));
                            self.logs.error(
                                &mut self.vm.logs,
                                format!("提资前更新（{kind}）没有排上"),
                                &error,
                                None,
                            );
                        }
                    }
                }
                match failed {
                    Some(reason) => self.data_handover_state.update_failed(&reason),
                    None => self.data_handover_state.update_queued(),
                }
                // 别等满一拍：刚排上的行尽快出现在队列快照里。
                self.poll_queue_now();
            }
            Evt::Preview(result) => self
                .data_handover_state
                .adopt_preview(result.map_err(|error| logs::error_chain(&error))),
            Evt::Deletions(result) => {
                if let Err(error) = &result {
                    let message = format!("获取待删除提资失败：{}", logs::error_chain(error));
                    eprintln!("[data_handover_delete_lookup] {message}");
                    self.logs.error(&mut self.vm.logs, message, error, None);
                }
                self.data_handover_state
                    .adopt_deletions(result.map_err(|error| logs::error_chain(&error)));
            }
            Evt::Published(request, result) => {
                self.data_handover_state.adopt_published(match &result {
                    Ok(response) => Ok(response.message.clone()),
                    Err(error) => Err(logs::error_chain(error)),
                });
                let label = format!(
                    "三维数据提资 · {}",
                    window::category_label(request.category)
                );
                self.handle_publish_result(ctx, &label, request.category.endpoint(), result);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_update_api::ApiError;

    fn api_error(code: &str, message: &str) -> anyhow::Error {
        anyhow::Error::new(ApiError(Failure::new(code, message)))
    }

    fn status(dbnum: u32) -> NodeModelStatus {
        NodeModelStatus {
            dbnum,
            verdict: "latest".into(),
            ..Default::default()
        }
    }

    /// 失败按码分型：元素已删除、没查成、整次不适用——三种出路各不相同，不许混。
    #[test]
    fn failures_are_told_apart_by_code() {
        let outcome = outcome_of(
            vec![
                (RefU64::from(1), Ok(status(8000))),
                (
                    RefU64::from(2),
                    Err(api_error("REFNO_NOT_FOUND", "元素不存在")),
                ),
                (
                    RefU64::from(3),
                    Err(api_error("timeout", "io: Connection refused")),
                ),
                (RefU64::from(4), Err(anyhow::anyhow!("解析响应失败"))),
                (
                    RefU64::from(5),
                    Err(api_error("SOURCE_NOT_FOUND", "库文件打不开")),
                ),
            ],
            "12:00:00".into(),
        );
        let Outcome::Checked { elements, at } = outcome else {
            panic!("该是一次查成了的检查");
        };
        assert_eq!(at, "12:00:00");
        assert!(matches!(&elements[0].fact, ElementFact::Checked(status) if status.dbnum == 8000));
        assert!(matches!(elements[1].fact, ElementFact::Gone));
        // 连不上：原文是传输层的英文，前面垫一句人话；服务端自己说的原因原话照登。
        assert!(matches!(
            &elements[2].fact,
            ElementFact::Failed(reason) if reason == "连不上模型服务，或请求超时（io: Connection refused）"
        ));
        assert!(matches!(elements[3].fact, ElementFact::Failed(_)));
        assert!(
            matches!(&elements[4].fact, ElementFact::Failed(reason) if reason == "库文件打不开")
        );
        assert_eq!(
            elements.iter().map(|e| e.refno).collect::<Vec<_>>(),
            (1..=5).map(RefU64::from).collect::<Vec<_>>(),
            "顺序与问的顺序一致"
        );
    }

    /// 老服务端没有这条路由：整次检查不适用，提资照旧、不拦。带包封的 404（元素不存在）不算。
    #[test]
    fn a_missing_route_makes_the_whole_check_unsupported() {
        let outcome = outcome_of(
            vec![
                (RefU64::from(1), Ok(status(8000))),
                (
                    RefU64::from(2),
                    Err(api_error(model_update_api::ROUTE_MISSING, "HTTP 404: ")),
                ),
            ],
            "12:00:00".into(),
        );
        assert!(matches!(outcome, Outcome::Unsupported(_)));
    }

    #[test]
    fn an_unsent_check_still_answers() {
        let Outcome::Checked { elements, .. } = unsent(vec![RefU64::from(7)], WORKER_DOWN) else {
            panic!("该是一次有回音的检查");
        };
        assert!(matches!(&elements[0].fact, ElementFact::Failed(reason) if reason == WORKER_DOWN));
    }
}
