//! Tauri mock-runtime IPC tests (the "Tauri application integration" layer of
//! SPEC §21.2). They exercise the real command handlers, argument parsing and
//! the native incarnation check through `tauri::test`. They do not prove the
//! capability ACL: the mock context has no application manifest, so the
//! native ACL probe in the actual application is the evidence for that.

use std::sync::Arc;

use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;
use tauri::{App, WebviewWindow, WebviewWindowBuilder};

use crate::bridge::Bridge;
use crate::launch::LaunchOptions;
use crate::{commands, window};

/// An identifier with no companion locator, so the companion is unavailable.
const TEST_IDENTIFIER: &str = "ai.scalinity.threadspace.mocktest";

fn app() -> (App<MockRuntime>, Arc<Bridge>) {
    let bridge = Arc::new(Bridge::new(TEST_IDENTIFIER.into()));
    let app = mock_builder()
        .manage(Arc::clone(&bridge))
        .invoke_handler(tauri::generate_handler![
            commands::ui_connect,
            commands::ui_ack,
            commands::ui_disconnect,
            commands::ui_query,
            commands::ui_action
        ])
        .build(mock_context(noop_assets()))
        .expect("mock app");
    window::create_office(app.handle(), &bridge, &LaunchOptions::default()).expect("office view");
    (app, bridge)
}

fn invoke(view: &WebviewWindow<MockRuntime>, command: &str, body: Value) -> Result<Value, Value> {
    get_ipc_response(
        view,
        InvokeRequest {
            cmd: command.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "tauri://localhost".parse().expect("url"),
            body: InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|response| response.deserialize::<Value>().expect("json reply"))
}

fn office(app: &App<MockRuntime>) -> WebviewWindow<MockRuntime> {
    use tauri::Manager;
    app.get_webview_window(window::OFFICE_LABEL)
        .expect("office")
}

fn code(error: &Value) -> &str {
    error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("<untyped>")
}

#[test]
fn bootstrap_status_is_a_typed_success_without_a_companion() {
    let (app, _bridge) = app();
    let reply = invoke(
        &office(&app),
        "ui_query",
        json!({ "request": { "query": { "kind": "ConnectionStatus" }, "context": null } }),
    )
    .expect("typed success");
    assert_eq!(reply["kind"], "ConnectionStatus");
    assert_eq!(reply["appIdentifier"], TEST_IDENTIFIER);
    assert_eq!(reply["companion"]["state"], "Unavailable");
}

#[test]
fn malformed_requests_are_typed_invalid_request() {
    let (app, _bridge) = app();
    let view = office(&app);
    let extra = invoke(
        &view,
        "ui_query",
        json!({ "request": { "query": { "kind": "Diagnostics", "sql": "x" }, "context": null } }),
    );
    assert_eq!(code(&extra.expect_err("rejected")), "INVALID_REQUEST");
    let wrong_type = invoke(
        &view,
        "ui_ack",
        json!({ "request": { "subscriptionId": 7 } }),
    );
    assert_eq!(code(&wrong_type.expect_err("rejected")), "INVALID_REQUEST");
}

#[test]
fn later_milestone_queries_are_typed_not_implemented() {
    let (app, _bridge) = app();
    let reply = invoke(
        &office(&app),
        "ui_query",
        json!({ "request": { "query": { "kind": "SessionDetail" }, "context": null } }),
    );
    assert_eq!(
        code(&reply.expect_err("not implemented")),
        "NOT_IMPLEMENTED_FOR_MILESTONE"
    );
}

#[test]
fn pages_require_a_subscription_context() {
    let (app, _bridge) = app();
    let reply = invoke(
        &office(&app),
        "ui_query",
        json!({ "request": { "query": { "kind": "FleetPage", "after": null, "limit": 10 }, "context": null } }),
    );
    assert_eq!(code(&reply.expect_err("needs context")), "INVALID_REQUEST");
}

#[test]
fn connect_reports_companion_unavailable() {
    let (app, _bridge) = app();
    let reply = invoke(
        &office(&app),
        "ui_connect",
        json!({ "request": { "protocolVersion": 1, "viewEpoch": "6f1b3c2e-4a5d-4e6f-8a9b-0c1d2e3f4a5b" }, "events": "__CHANNEL__:7" }),
    );
    let error = reply.expect_err("no companion");
    assert_eq!(code(&error), "COMPANION_UNAVAILABLE");
    assert_eq!(error["retryable"], true);
}

#[test]
fn a_view_without_the_native_marker_is_stale() {
    let (app, _bridge) = app();
    let other = WebviewWindowBuilder::new(&app, "other", Default::default())
        .build()
        .expect("second view");
    let reply = invoke(
        &other,
        "ui_query",
        json!({ "request": { "query": { "kind": "ConnectionStatus" }, "context": null } }),
    );
    assert_eq!(code(&reply.expect_err("stale")), "STALE_VIEW");
}
