//! Cleanup of qualification side effects on an installed identity: the
//! companion's own notifications, and attention items the qualification
//! created (resolved explicitly, with a reason, through the normal journaled
//! owner command — never deleted).

use std::time::Duration;

use serde_json::{Value, json};
use threadspace_contracts::control::{ControlRequestBody, ControlResponseBody};

use crate::ctx::Ctx;

pub fn clear_notifications(ctx: &Ctx) -> Result<Value, String> {
    match ctx.companion().request(
        ControlRequestBody::QualifyClearNotifications,
        Duration::from_secs(20),
    )? {
        ControlResponseBody::NotificationsCleared { removed } => {
            Ok(json!({ "removedDelivered": removed }))
        }
        other => Err(format!("unexpected {other:?}")),
    }
}

pub fn resolve_qualification(ctx: &Ctx, reason: &str) -> Result<Value, String> {
    let mut client = ctx.companion().client(Duration::from_secs(30))?;
    let mut targets = Vec::new();
    let mut after = None;
    loop {
        match client
            .request(ControlRequestBody::AttentionPage {
                after: after.clone(),
                limit: 500,
            })
            .map_err(|e| e.to_string())?
        {
            ControlResponseBody::AttentionPage { page } => {
                targets.extend(
                    page.rows
                        .iter()
                        .filter(|row| {
                            row.summary
                                .as_deref()
                                .is_some_and(|s| s.starts_with("Qualification turn completed"))
                        })
                        .map(|row| row.attention_id.clone()),
                );
                after = page.next_after;
                if after.is_none() {
                    break;
                }
            }
            other => return Err(format!("unexpected {other:?}")),
        }
    }
    let mut resolved = 0;
    let mut failed = 0;
    for attention_id in &targets {
        let reply = client.request(ControlRequestBody::ResolveAttention {
            command_id: uuid::Uuid::new_v4().to_string(),
            attention_id: attention_id.clone(),
            expected_revision: None,
            reason: reason.to_owned(),
        });
        if matches!(reply, Ok(ControlResponseBody::CommandReceipt { .. })) {
            resolved += 1;
        } else {
            failed += 1;
        }
    }
    Ok(
        json!({ "matched": targets.len(), "resolved": resolved, "failed": failed, "reason": reason }),
    )
}
