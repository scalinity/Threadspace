//! Notification-response Returns (SPEC §7.5), run off the writer thread
//! because a verified route takes native time. The result travels back to the
//! views as an inspector intent; the containing application is brought
//! forward only when the route did not reach an exact current surface, so a
//! successful Return is not undone by activating Threadspace.

use std::sync::mpsc::{Receiver, SyncSender};
use std::thread;
use std::time::Instant;

use serde_json::json;
use threadspace_contracts::projection::{IntentAction, IntentSource, NativeIntent};
use threadspace_contracts::route::{
    RouteRequest, RouteSummary, SessionVerification, SurfaceResult,
};
use uuid::Uuid;

use crate::discovery::{DiscoveryContext, Trigger};
use crate::log;
use crate::route;
use crate::writer::WriterCommand;

pub enum ResponseJob {
    Return {
        notification_request_id: String,
        attention_id: String,
        session_id: String,
    },
}

pub fn spawn(
    jobs: Receiver<ResponseJob>,
    writer: SyncSender<WriterCommand>,
    claude: DiscoveryContext,
    discovery: SyncSender<Trigger>,
) -> std::io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name("notification-return".into())
        .spawn(move || {
            for job in jobs {
                let ResponseJob::Return {
                    notification_request_id,
                    attention_id,
                    session_id,
                } = job;
                let received = Instant::now();
                let request = RouteRequest {
                    request_id: Uuid::new_v4().to_string(),
                    session_id: session_id.clone(),
                    chosen_binding_id: None,
                    expected_binding_revision: None,
                };
                let outcome = route::return_to_session(request, &claude, &discovery, received);
                let (summary, exact) = match outcome {
                    Ok(result) => {
                        let exact = result.surface_result == SurfaceResult::ExactNativeSurface
                            && result.session_verification
                                == SessionVerification::CurrentNativeRevalidated;
                        (
                            Some(RouteSummary {
                                request_id: result.request_id,
                                surface_result: result.surface_result,
                                session_verification: result.session_verification,
                                input_readiness: result.input_readiness,
                                reason_code: result.reason_code,
                                focus_performed: result.focus_performed,
                                latency_ms: result.latency_ms,
                                recorded_at_ms: log::now_ms(),
                            }),
                            exact,
                        )
                    }
                    Err(error) => {
                        log::warn(
                            "NOTIFICATION_RETURN_REFUSED",
                            json!({ "attentionId": attention_id, "error": error.detail }),
                        );
                        (None, false)
                    }
                };
                log::info(
                    "NOTIFICATION_RETURN",
                    json!({
                        "notificationRequestId": notification_request_id,
                        "attentionId": attention_id,
                        "sessionId": session_id,
                        "exact": exact,
                        "route": summary,
                    }),
                );
                let intent = NativeIntent {
                    intent_id: Uuid::new_v4().to_string(),
                    action: IntentAction::OpenAttention {
                        attention_id,
                        session_id,
                        outstanding: true,
                        source: IntentSource::NotificationResponse,
                        route: summary,
                        observation_enabled: true,
                    },
                };
                if writer
                    .send(WriterCommand::PushIntent {
                        intent,
                        open_app: !exact,
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
}
