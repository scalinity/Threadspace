//! The five renderer bridge commands (SPEC §18.3). Each verifies the calling
//! WebView's native incarnation marker first, then parses its raw argument
//! into a strict contract type so malformed input becomes `INVALID_REQUEST`.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{Manager, Runtime, State, Webview};
use threadspace_contracts::control::{ControlRequestBody, ControlResponseBody};
use threadspace_contracts::cursor::parse_cursor;
use threadspace_contracts::limits::QUERY_REPLY_MAX_BYTES;
use threadspace_contracts::route::RouteRequest;
use threadspace_contracts::ui::{
    CompanionLink, ConnectionStatus, DiagnosticsReport, IntegrationReport, UI_PROTOCOL_VERSION,
    UiAckReply, UiAckRequest, UiAction, UiActionRequest, UiActionResult, UiConnectReply,
    UiConnectRequest, UiDisconnectRequest, UiError, UiErrorCode, UiFrame, UiQuery, UiQueryRequest,
    UiQueryResult, parse_request, parse_uuid,
};

use crate::bootstrap;
use crate::bridge::{Bridge, link_error};
use crate::diagnostics;

const QUICK: Duration = Duration::from_secs(5);
/// A route has a two-second native budget; this bounds the whole round trip,
/// including a wait behind another route.
const ROUTE: Duration = Duration::from_secs(15);
/// One discovery pass, including a Terminal surface join.
const DISCOVERY: Duration = Duration::from_secs(35);
/// Upper bound on the owner answering a native permission prompt.
const PROMPT: Duration = Duration::from_secs(190);

#[tauri::command]
pub async fn ui_connect<R: Runtime>(
    webview: Webview<R>,
    bridge: State<'_, Arc<Bridge>>,
    request: Value,
    events: Channel<UiFrame>,
) -> Result<UiConnectReply, UiError> {
    let incarnation = bridge.views.verify(&webview)?;
    let request: UiConnectRequest = parse_request(request)?;
    bridge
        .inner()
        .connect_view(incarnation, request, events)
        .await
}

#[tauri::command]
pub async fn ui_ack<R: Runtime>(
    webview: Webview<R>,
    bridge: State<'_, Arc<Bridge>>,
    request: Value,
) -> Result<UiAckReply, UiError> {
    let incarnation = bridge.views.verify(&webview)?;
    let request: UiAckRequest = parse_request(request)?;
    bridge.inner().ack(incarnation, &request)
}

#[tauri::command]
pub async fn ui_disconnect<R: Runtime>(
    webview: Webview<R>,
    bridge: State<'_, Arc<Bridge>>,
    request: Value,
) -> Result<(), UiError> {
    let incarnation = bridge.views.verify(&webview)?;
    let request: UiDisconnectRequest = parse_request(request)?;
    bridge.inner().disconnect(incarnation, &request)
}

fn bounded<T: serde::Serialize>(
    bridge: &Bridge,
    incarnation: uuid::Uuid,
    value: T,
) -> Result<T, UiError> {
    let size = serde_json::to_vec(&value)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX);
    bridge.note_reply(incarnation, size);
    if size > QUERY_REPLY_MAX_BYTES {
        return Err(UiError::new(
            UiErrorCode::ReplyTooLarge,
            format!("reply of {size} bytes exceeds 64 KiB"),
        ));
    }
    Ok(value)
}

#[tauri::command]
pub async fn ui_query<R: Runtime>(
    webview: Webview<R>,
    bridge: State<'_, Arc<Bridge>>,
    request: Value,
) -> Result<UiQueryResult, UiError> {
    let incarnation = bridge.views.verify(&webview)?;
    let request: UiQueryRequest = parse_request(request)?;
    let bridge = bridge.inner();
    let _slot = bridge.acquire_query_slot()?;
    let subscribed = |bridge: &Arc<Bridge>| -> Result<(), UiError> {
        let context = request
            .context
            .as_ref()
            .ok_or_else(|| UiError::invalid("this query requires a subscription context"))?;
        bridge.validate_context(incarnation, context).map(|_| ())
    };
    let result = match &request.query {
        UiQuery::ConnectionStatus {} => {
            // Bootstrap status: context is optional but must be valid if given.
            if request.context.is_some() {
                subscribed(bridge)?;
            }
            let companion = match bridge.link() {
                Ok(link) => CompanionLink::Connected {
                    core_generation: link.hello.core_generation.clone(),
                    store_generation: link.hello.store_generation.clone(),
                    companion_pid: link.hello.companion.pid,
                },
                Err(error) => CompanionLink::Unavailable {
                    reason: error.detail,
                },
            };
            UiQueryResult::ConnectionStatus(ConnectionStatus {
                ui_protocol_version: UI_PROTOCOL_VERSION,
                app_identifier: bridge.app_identifier.clone(),
                companion,
                active_subscriptions: bridge.subscription_count() as u32,
            })
        }
        UiQuery::Diagnostics {} => {
            subscribed(bridge)?;
            let version = webview.app_handle().package_info().version.to_string();
            let companion = match bridge
                .link()?
                .request(ControlRequestBody::Diagnostics, QUICK)
                .await
            {
                Ok(ControlResponseBody::Diagnostics { report }) => Some(*report),
                _ => None,
            };
            UiQueryResult::Diagnostics(Box::new(DiagnosticsReport {
                desktop: diagnostics::desktop(&bridge.app_identifier, &version),
                companion,
            }))
        }
        UiQuery::IntegrationStatus {} => {
            subscribed(bridge)?;
            let service = bootstrap::report(&bridge.agent_identifier());
            let companion = match bridge
                .link()?
                .request(ControlRequestBody::IntegrationStatus, QUICK)
                .await
            {
                Ok(ControlResponseBody::IntegrationStatus { report }) => Some(report),
                _ => None,
            };
            UiQueryResult::IntegrationStatus(Box::new(IntegrationReport { service, companion }))
        }
        UiQuery::FleetPage { after, limit } | UiQuery::AttentionPage { after, limit } => {
            let subscription = bridge.validate_context(
                incarnation,
                request.context.as_ref().ok_or_else(|| {
                    UiError::invalid("this query requires a subscription context")
                })?,
            )?;
            if let Some(after) = after {
                parse_uuid(after, "after")?;
            }
            if *limit == 0 || *limit > 500 {
                return Err(UiError::invalid("limit must be 1-500"));
            }
            let link = bridge.link()?;
            if link.id != subscription.link_id {
                return Err(UiError::new(
                    UiErrorCode::StaleContext,
                    "companion connection changed",
                ));
            }
            let attention = matches!(request.query, UiQuery::AttentionPage { .. });
            let body = if attention {
                ControlRequestBody::AttentionPage {
                    after: after.clone(),
                    limit: *limit,
                }
            } else {
                ControlRequestBody::FleetPage {
                    after: after.clone(),
                    limit: *limit,
                }
            };
            let reply = link.request(body, QUICK).await.map_err(link_error)?;
            // The view may have been retired while the page was read; a stale
            // reply never reaches the new view (SPEC §18.3).
            bridge.validate_context(
                incarnation,
                request
                    .context
                    .as_ref()
                    .ok_or_else(|| UiError::invalid("context"))?,
            )?;
            match reply {
                ControlResponseBody::FleetPage { page } => UiQueryResult::FleetPage(page),
                ControlResponseBody::AttentionPage { page } => UiQueryResult::AttentionPage(page),
                _ => {
                    return Err(UiError::new(
                        UiErrorCode::Internal,
                        "unexpected companion reply",
                    ));
                }
            }
        }
        UiQuery::WindowState {} => UiQueryResult::WindowState(crate::window::state_of(&webview)?),
        UiQuery::SessionDetail {} | UiQuery::ProjectDetail {} => {
            return Err(UiError::not_implemented(
                &format!("{:?}", request.query).replace(" {}", ""),
            ));
        }
    };
    bounded(bridge, incarnation, result)
}

#[tauri::command]
pub async fn ui_action<R: Runtime>(
    webview: Webview<R>,
    bridge: State<'_, Arc<Bridge>>,
    request: Value,
) -> Result<UiActionResult, UiError> {
    let incarnation = bridge.views.verify(&webview)?;
    let request: UiActionRequest = parse_request(request)?;
    parse_uuid(&request.request_id, "requestId")?;
    let bridge = bridge.inner();
    // Observation enable/stop run the outer bootstrap ordering (SPEC §19.5)
    // and must work while no companion is reachable; they need only the
    // verified current view.
    if let UiAction::EnableObservation {} | UiAction::StopObservation {} = request.action {
        let enable = matches!(request.action, UiAction::EnableObservation {});
        let agent = bridge.agent_identifier();
        let (ok, detail) = tauri::async_runtime::spawn_blocking(move || {
            if enable {
                bootstrap::enable(&agent)
            } else {
                bootstrap::stop(&agent)
            }
        })
        .await
        .map_err(|error| UiError::new(UiErrorCode::Internal, error.to_string()))?;
        let service = bootstrap::report(&bridge.agent_identifier());
        let summary: String = detail.to_string().chars().take(240).collect();
        if !ok {
            return Err(UiError::new(UiErrorCode::CompanionRejected, summary));
        }
        return Ok(UiActionResult::ObservationChanged {
            enabled: enable,
            service,
            detail: summary,
        });
    }
    let subscription = bridge.validate_context(incarnation, &request.context)?;
    let link = bridge.link()?;
    if link.id != subscription.link_id {
        return Err(UiError::new(
            UiErrorCode::StaleContext,
            "companion connection changed",
        ));
    }
    let result = match request.action {
        UiAction::AcknowledgeAttention { attention_id } => {
            parse_uuid(&attention_id, "attentionId")?;
            if let Some(revision) = &request.expected_revision
                && parse_cursor(revision).is_none()
            {
                return Err(UiError::invalid(
                    "expectedRevision must be a canonical cursor",
                ));
            }
            let body = ControlRequestBody::AcknowledgeAttention {
                command_id: request.request_id,
                attention_id,
                expected_revision: request.expected_revision,
            };
            match link.request(body, QUICK).await.map_err(link_error)? {
                ControlResponseBody::CommandReceipt { receipt } => {
                    UiActionResult::CommandCommitted { receipt }
                }
                _ => {
                    return Err(UiError::new(
                        UiErrorCode::Internal,
                        "unexpected companion reply",
                    ));
                }
            }
        }
        UiAction::ResolveAttention {
            attention_id,
            reason,
        } => {
            parse_uuid(&attention_id, "attentionId")?;
            if reason.trim().is_empty() {
                return Err(UiError::invalid("a resolution reason is required"));
            }
            if let Some(revision) = &request.expected_revision
                && parse_cursor(revision).is_none()
            {
                return Err(UiError::invalid(
                    "expectedRevision must be a canonical cursor",
                ));
            }
            let body = ControlRequestBody::ResolveAttention {
                command_id: request.request_id,
                attention_id,
                expected_revision: request.expected_revision,
                reason,
            };
            match link.request(body, QUICK).await.map_err(link_error)? {
                ControlResponseBody::CommandReceipt { receipt } => {
                    UiActionResult::CommandCommitted { receipt }
                }
                _ => {
                    return Err(UiError::new(
                        UiErrorCode::Internal,
                        "unexpected companion reply",
                    ));
                }
            }
        }
        UiAction::RequestNotificationAuthorization {} => {
            match link
                .request(ControlRequestBody::RequestNotificationAuthorization, PROMPT)
                .await
                .map_err(link_error)?
            {
                ControlResponseBody::NotificationAuthorization { granted, settings } => {
                    UiActionResult::NotificationAuthorization { granted, settings }
                }
                _ => {
                    return Err(UiError::new(
                        UiErrorCode::Internal,
                        "unexpected companion reply",
                    ));
                }
            }
        }
        UiAction::RequestTerminalAutomation {} => {
            match link
                .request(ControlRequestBody::RequestTerminalAutomation, PROMPT)
                .await
                .map_err(link_error)?
            {
                ControlResponseBody::TerminalAutomation {
                    automation,
                    status_code,
                } => UiActionResult::TerminalAutomation {
                    automation,
                    status_code,
                },
                _ => {
                    return Err(UiError::new(
                        UiErrorCode::Internal,
                        "unexpected companion reply",
                    ));
                }
            }
        }
        UiAction::ReturnToSession {
            session_id,
            chosen_binding_id,
            expected_binding_revision,
        } => {
            parse_uuid(&session_id, "sessionId")?;
            if let Some(binding) = &chosen_binding_id {
                parse_uuid(binding, "chosenBindingId")?;
            }
            if let Some(revision) = &expected_binding_revision
                && parse_cursor(revision).is_none()
            {
                return Err(UiError::invalid(
                    "expectedBindingRevision must be a canonical cursor",
                ));
            }
            let body = ControlRequestBody::ReturnToSession {
                route: RouteRequest {
                    request_id: request.request_id,
                    session_id,
                    chosen_binding_id,
                    expected_binding_revision,
                },
            };
            match link.request(body, ROUTE).await.map_err(link_error)? {
                ControlResponseBody::Routed { result } => UiActionResult::Routed { result },
                _ => {
                    return Err(UiError::new(
                        UiErrorCode::Internal,
                        "unexpected companion reply",
                    ));
                }
            }
        }
        UiAction::RefreshEvidence {} => {
            match link
                .request(ControlRequestBody::RefreshEvidence, DISCOVERY)
                .await
                .map_err(link_error)?
            {
                ControlResponseBody::EvidenceRefreshed { summary } => {
                    UiActionResult::EvidenceRefreshed { summary }
                }
                _ => {
                    return Err(UiError::new(
                        UiErrorCode::Internal,
                        "unexpected companion reply",
                    ));
                }
            }
        }
        #[cfg(feature = "qualification")]
        UiAction::RecordQualificationReport {
            report_kind,
            report,
        } => {
            let file_name =
                crate::qualification::record(webview.app_handle(), &report_kind, &report)?;
            UiActionResult::QualificationReportRecorded { file_name }
        }
        other => {
            let kind = serde_json::to_value(&other)
                .ok()
                .and_then(|value| value.get("kind").and_then(Value::as_str).map(str::to_owned))
                .unwrap_or_default();
            return Err(UiError::not_implemented(&kind));
        }
    };
    Ok(result)
}
