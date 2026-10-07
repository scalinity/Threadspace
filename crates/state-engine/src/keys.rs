//! Native identity keys (SPEC §4.1, §4.2): the unambiguous strings admission
//! records against canonical IDs. Each is a JSON array, so no separator in a
//! native value can collide two keys. The namespaces are separate by
//! construction: a session key can never equal a turn, process or cursor key.

use threadspace_contracts::canonical::keys::{NativeActorRef, NativeSurfaceRef};
use threadspace_contracts::route::ProcessKey;

fn key(parts: &[&str]) -> String {
    serde_json::to_string(parts).unwrap_or_default()
}

pub fn namespace(provider: &str, endpoint_id: &str, profile_ref: &str) -> String {
    key(&["namespace", provider, endpoint_id, profile_ref])
}

pub fn session(namespace_id: &str, native_session_id: &str) -> String {
    key(&["session", namespace_id, native_session_id])
}

pub fn actor(session_id: &str, actor: &NativeActorRef) -> String {
    match actor {
        NativeActorRef::Principal => key(&["actor", session_id, "principal"]),
        NativeActorRef::Agent { native_agent_id } => {
            key(&["actor", session_id, "agent", native_agent_id])
        }
    }
}

pub fn process(process: &ProcessKey) -> String {
    key(&[
        "process",
        &process.endpoint_id,
        &process.boot_id,
        &process.pid.to_string(),
        &process.start_seconds,
        &process.start_microseconds.to_string(),
    ])
}

pub fn execution(session_id: &str, actor_id: &str, activation_ref: &str) -> String {
    key(&["execution", session_id, actor_id, activation_ref])
}

pub fn turn(session_id: &str, actor_id: &str, native_turn_id: &str) -> String {
    key(&["turn", session_id, actor_id, native_turn_id])
}

pub fn input(session_id: &str, native_input_key: &str) -> String {
    key(&["input", session_id, native_input_key])
}

pub fn activity(session_id: &str, native_occurrence_id: &str) -> String {
    key(&["activity", session_id, native_occurrence_id])
}

pub fn surface(endpoint_id: &str, surface: &NativeSurfaceRef) -> String {
    let device = surface.device_number.map(|d| d.to_string()).unwrap_or_default();
    key(&[
        "surface",
        endpoint_id,
        &surface.surface_kind,
        &surface.app_generation,
        &surface.locator,
        &device,
        &surface.surface_generation,
    ])
}

pub fn binding(execution_id: &str, surface_id: &str) -> String {
    key(&["binding", execution_id, surface_id])
}

/// Exact native requests are keyed, not allocated.
pub fn request(session_id: &str, native_request_id: &str) -> String {
    key(&["request", session_id, native_request_id])
}

pub fn wait_scope(
    session_id: &str,
    actor_id: Option<&str>,
    execution_id: Option<&str>,
    category: &str,
    generation: Option<&str>,
) -> String {
    key(&[
        "wait",
        session_id,
        actor_id.unwrap_or(""),
        execution_id.unwrap_or(""),
        category,
        generation.unwrap_or(""),
    ])
}

pub fn frontier(session_id: &str, actor_id: &str, source_id: &str, epoch: &str, domain: &str) -> String {
    key(&["frontier", session_id, actor_id, source_id, epoch, domain])
}

pub fn coverage(source_id: &str, epoch: &str) -> String {
    key(&["coverage", source_id, epoch])
}

/// The attention scope key: unique per Session and scope (SPEC §7.1).
pub fn attention_scope(session_id: &str, scope_kind: &str, scope_key: &str) -> String {
    key(&["attention", session_id, scope_kind, scope_key])
}
