//! Native ownership probe; the record adapter is portable and pure.

pub use crate::ownership_record::*;
use threadspace_contracts::canonical::envelope::{ProcessRole, ProcessSample};
use threadspace_surfaces_macos::ancestry::Sampler;
use crate::discovery::discover;
use crate::inventory::Inventory;
use crate::profiles::{observer_profile, version_from_executable};

/// Performs the bounded native join. The returned image is the actual
/// incarnation from the helper ancestry, unchanged throughout the bracket.
pub fn corroborate(
    inventory: &impl Inventory,
    kernel: &impl Sampler,
    provider: &ProcessSample,
    request: &ProbeRequest,
) -> Result<(String, i64, i64), &'static str> {
    if !request.valid() || provider.role != ProcessRole::Provider || provider.key.boot_id.is_empty() {
        return Err("INVALID_SCOPE");
    }
    let executable = provider.executable.as_deref().ok_or("NO_EXECUTABLE")?;
    let version = version_from_executable(executable).ok_or("UNQUALIFIED_EXECUTABLE")?;
    if observer_profile(&version).is_none() {
        return Err("UNQUALIFIED_EXECUTABLE");
    }
    let pass = discover(inventory, kernel, |image| {
        version_from_executable(image).as_deref() == Some(version.as_str())
    }).map_err(|_| "INVENTORY_UNAVAILABLE")?;
    let join = pass.join_for_pid(provider.key.pid as i32).ok_or("NO_CURRENT_JOIN")?;
    if join.native_session_id != request.session_id {
        return Err("CURRENT_SESSION_MISMATCH");
    }
    let after = &join.after;
    if after.sample.start_seconds.to_string() != provider.key.start_seconds
        || after.sample.start_microseconds != provider.key.start_microseconds
        || after.executable.canonical() != executable
        || join.before.executable.canonical() != executable
    {
        return Err("PROCESS_OR_EXECUTABLE_CHANGED");
    }
    let second = pass.second.as_ref().ok_or("NO_CONFIRMATION")?;
    Ok((executable.to_owned(), pass.first.request_started_ms, second.request_ended_ms))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use threadspace_contracts::route::ProcessKey;
    use crate::discovery::tests::{Kernel, Script, claude, row, snapshot};
    use super::*;

    fn request(session: &str) -> ProbeRequest {
        ProbeRequest { protocol_version: 1, source_epoch: "reload-epoch".into(), session_generation: 3, session_id: session.into() }
    }

    fn provider() -> ProcessSample {
        ProcessSample {
            role: ProcessRole::Provider,
            key: ProcessKey { endpoint_id: String::new(), boot_id: "boot".into(), pid: 11, start_seconds: "1000".into(), start_microseconds: 7 },
            parent_pid: Some(100), executable: Some(claude(11, 1000, Some(5)).executable.canonical()), controlling_device: Some(5),
        }
    }

    fn run(session: &str, claim: &str, provider: &ProcessSample) -> Result<(String, i64, i64), &'static str> {
        let rows = vec![row(11, session)];
        let inventory = Script(RefCell::new(vec![Ok(snapshot(rows.clone())), Ok(snapshot(rows))].into()));
        let kernel = Kernel::new(vec![(11, vec![Ok(claude(11, 1000, Some(5)))] )]);
        corroborate(&inventory, &kernel, provider, &request(claim))
    }

    #[test]
    fn current_native_join_can_prove_unchanged_session_but_not_historical_claim() {
        assert!(run("A", "A", &provider()).is_ok());
        assert_eq!(run("B", "A", &provider()), Err("CURRENT_SESSION_MISMATCH"));
        assert!(run("B", "B", &provider()).is_ok());
    }

    #[test]
    fn same_pid_wrong_birth_or_executable_has_no_proof() {
        let mut changed = provider();
        changed.key.start_seconds = "999".into();
        assert_eq!(run("A", "A", &changed), Err("PROCESS_OR_EXECUTABLE_CHANGED"));
        changed = provider();
        changed.executable = Some(changed.executable.expect("image").replace("#16777234:42", "#16777234:43"));
        assert_eq!(run("A", "A", &changed), Err("PROCESS_OR_EXECUTABLE_CHANGED"));
        changed.executable = Some("/h/.local/share/claude/versions/2.1.292#1:2".into());
        assert_eq!(run("A", "A", &changed), Err("UNQUALIFIED_EXECUTABLE"));
    }

    #[test]
    fn session_switch_during_probe_is_not_continuous_ownership() {
        let inventory = Script(RefCell::new(vec![Ok(snapshot(vec![row(11, "A")])), Ok(snapshot(vec![row(11, "B")]))].into()));
        let kernel = Kernel::new(vec![(11, vec![Ok(claude(11, 1000, Some(5)))])]);
        assert_eq!(corroborate(&inventory, &kernel, &provider(), &request("A")), Err("NO_CURRENT_JOIN"));
    }

    #[test]
    fn actual_295_image_qualifies_but_a_switch_between_two_qualified_images_does_not() {
        let mut image = claude(11, 1000, Some(5));
        image.executable.path = image.executable.path.replace("2.1.291", "2.1.295");
        let mut parent = provider();
        parent.executable = Some(image.executable.canonical());
        let rows = vec![row(11, "A")];
        let inventory = Script(RefCell::new(vec![Ok(snapshot(rows.clone())), Ok(snapshot(rows.clone()))].into()));
        let kernel = Kernel::new(vec![(11, vec![Ok(image.clone())])]);
        assert!(corroborate(&inventory, &kernel, &parent, &request("A")).is_ok());
        let inventory = Script(RefCell::new(vec![Ok(snapshot(rows.clone())), Ok(snapshot(rows))].into()));
        let kernel = Kernel::new(vec![(11, vec![Ok(image)])]);
        assert_eq!(corroborate(&inventory, &kernel, &provider(), &request("A")), Err("NO_CURRENT_JOIN"));
    }
}
