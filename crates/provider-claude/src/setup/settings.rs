//! Pure edits of a Claude `settings.json` document (SPEC §19.2). Owned
//! entries are identified by their command string and plugin directory,
//! appended after everything foreign, and removed only while their installed
//! structure still matches. A marker identifies a possible owned entry; it
//! does not authorize deleting an owner-modified matcher, hook or group.

use super::OwnedEntry;
use super::ordered_json::{Map, Value};

pub const PLUGIN_DIRS: &str = "CLAUDE_CODE_PLUGIN_DIRS";

/// What `remove` did and could not do.
#[derive(Debug, Default)]
pub struct Removal {
    pub removed: Vec<OwnedEntry>,
    /// Owned entries no longer found as installed; left untouched.
    pub conflicts: Vec<OwnedEntry>,
    pub changed: bool,
}

/// Locations of remaining possible owned-resource references. Values are not
/// exported: an owner command may contain private arguments. This is a
/// conservative retention check, never a shell interpreter or delete grant.
pub fn reference_locations(root: &Map, needles: &[String]) -> Vec<String> {
    fn visit(value: &Value, pointer: &str, needles: &[String], found: &mut Vec<String>) {
        match value {
            Value::String(text) if needles.iter().any(|needle| text.contains(needle)) => {
                found.push(pointer.to_owned());
            }
            Value::Array(values) => {
                for (index, value) in values.iter().enumerate() {
                    visit(value, &format!("{pointer}/{index}"), needles, found);
                }
            }
            Value::Object(fields) => walk(fields, pointer, needles, found),
            _ => {}
        }
    }
    fn walk(fields: &Map, pointer: &str, needles: &[String], found: &mut Vec<String>) {
        for key in fields.keys() {
            if let Some(value) = fields.get(key) {
                let escaped = key.replace('~', "~0").replace('/', "~1");
                visit(value, &format!("{pointer}/{escaped}"), needles, found);
            }
        }
    }
    let mut found = Vec::new();
    walk(root, "", needles, &mut found);
    found
}

/// Adds every owned entry the document does not already hold. Plugin
/// directories under `stale_prefix` (earlier owned mod copies) are replaced.
/// Returns the containers it created, as `/hooks`, `/hooks/<event>`, `/env`
/// and `/env/CLAUDE_CODE_PLUGIN_DIRS`. A container of the wrong type stops
/// the edit, because merging into it could not be lossless.
pub fn add(
    root: &mut Map,
    entries: &[OwnedEntry],
    stale_prefix: &str,
) -> Result<Vec<String>, String> {
    let mut created = Vec::new();
    for entry in entries {
        match entry {
            OwnedEntry::Hook {
                event,
                matcher,
                command,
            } => {
                let hooks = object_at(root, "hooks", "/hooks", &mut created)?;
                let groups = array_at(hooks, event, &format!("/hooks/{event}"), &mut created)?;
                if !groups.iter().any(|group| holds_command(group, command)) {
                    groups.push(group(matcher, command));
                }
            }
            OwnedEntry::PluginDir { path } => {
                let env = object_at(root, "env", "/env", &mut created)?;
                let list = match env.get(PLUGIN_DIRS) {
                    None => {
                        created.push(format!("/env/{PLUGIN_DIRS}"));
                        String::new()
                    }
                    Some(Value::String(list)) => list.clone(),
                    Some(_) => return Err(format!("`/env/{PLUGIN_DIRS}` is not a string")),
                };
                let mut parts = split(&list);
                parts.retain(|part| *part == path || !part.starts_with(stale_prefix));
                if !parts.contains(&path.as_str()) {
                    parts.push(path);
                }
                env.insert(PLUGIN_DIRS, Value::string(parts.join(":")));
            }
        }
    }
    Ok(created)
}

/// Removes the owned entries that still match exactly, then any container
/// listed in `created` that is now empty.
pub fn remove(root: &mut Map, entries: &[OwnedEntry], created: &[String]) -> Removal {
    let was_created = |path: &str| created.iter().any(|item| item == path);
    let mut removal = Removal::default();
    for entry in entries {
        let found = match entry {
            OwnedEntry::Hook {
                event,
                matcher,
                command,
            } => remove_hook(
                root,
                event,
                matcher,
                command,
                was_created(&format!("/hooks/{event}")),
                &mut removal.changed,
            ),
            OwnedEntry::PluginDir { path } => remove_plugin_dir(
                root,
                path,
                was_created(&format!("/env/{PLUGIN_DIRS}")),
                &mut removal.changed,
            ),
        };
        if found {
            removal.removed.push(entry.clone());
        } else {
            removal.conflicts.push(entry.clone());
        }
    }
    for key in ["hooks", "env"] {
        let empty = root
            .get(key)
            .and_then(Value::as_object)
            .is_some_and(Map::is_empty);
        if empty && was_created(&format!("/{key}")) {
            root.remove(key);
            removal.changed = true;
        }
    }
    removal
}

fn remove_hook(
    root: &mut Map,
    event: &str,
    matcher: &str,
    command: &str,
    created_event: bool,
    changed: &mut bool,
) -> bool {
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return false;
    };
    let Some(groups) = hooks.get_mut(event).and_then(Value::as_array_mut) else {
        return false;
    };
    let mut found = false;
    groups.retain_mut(|group| {
        let Some(group) = group.as_object_mut() else {
            return true;
        };
        // The installed group has only matcher/hooks. Foreign sibling hooks
        // may be added, but a changed matcher or group option is a conflict.
        if group.get("matcher").and_then(Value::as_str) != Some(matcher)
            || group.keys().any(|key| key != "matcher" && key != "hooks")
        {
            return true;
        }
        let Some(list) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
            return true;
        };
        let before = list.len();
        list.retain(|hook| !unchanged_hook(hook, command));
        if list.len() == before {
            return true;
        }
        found = true;
        // A group goes only when our hook was all it held.
        let emptied = list.is_empty();
        !(emptied && group.keys().all(|key| key == "matcher" || key == "hooks"))
    });
    let empty = groups.is_empty();
    if empty && created_event {
        hooks.remove(event);
        *changed = true;
    }
    *changed |= found;
    found
}

/// Our complete installed hook shape (including the absence of timeout or
/// other options). JSON object key order does not change hook behavior.
fn unchanged_hook(hook: &Value, command: &str) -> bool {
    hook.as_object().is_some_and(|fields| {
        fields.get("command").and_then(Value::as_str) == Some(command)
            && fields.get("type").and_then(Value::as_str) == Some("command")
            && fields.keys().all(|key| key == "command" || key == "type")
    })
}

fn remove_plugin_dir(root: &mut Map, path: &str, created_key: bool, changed: &mut bool) -> bool {
    let Some(env) = root.get_mut("env").and_then(Value::as_object_mut) else {
        return false;
    };
    let Some(list) = env.get(PLUGIN_DIRS).and_then(Value::as_str) else {
        return false;
    };
    let parts = split(list);
    if !parts.contains(&path) {
        return false;
    }
    let rest: Vec<&str> = parts.into_iter().filter(|part| *part != path).collect();
    let joined = rest.join(":");
    if rest.is_empty() && created_key {
        env.remove(PLUGIN_DIRS);
    } else {
        env.insert(PLUGIN_DIRS, Value::string(joined));
    }
    *changed = true;
    true
}

/// The elements of a `:`-separated path list; an empty value has none.
fn split(list: &str) -> Vec<&str> {
    if list.is_empty() {
        Vec::new()
    } else {
        list.split(':').collect()
    }
}

fn holds_command(group: &Value, command: &str) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hooks| {
            hooks
                .iter()
                .any(|hook| hook.get("command").and_then(Value::as_str) == Some(command))
        })
}

fn group(matcher: &str, command: &str) -> Value {
    let hook: Map = [
        ("type".to_owned(), Value::string("command")),
        ("command".to_owned(), Value::string(command)),
    ]
    .into_iter()
    .collect();
    Value::Object(
        [
            ("matcher".to_owned(), Value::string(matcher)),
            ("hooks".to_owned(), Value::Array(vec![Value::Object(hook)])),
        ]
        .into_iter()
        .collect(),
    )
}

fn object_at<'a>(
    map: &'a mut Map,
    key: &str,
    path: &str,
    created: &mut Vec<String>,
) -> Result<&'a mut Map, String> {
    if map.get(key).is_none() {
        map.insert(key, Value::Object(Map::new()));
        created.push(path.to_owned());
    }
    map.get_mut(key)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("`{path}` is not an object"))
}

fn array_at<'a>(
    map: &'a mut Map,
    key: &str,
    path: &str,
    created: &mut Vec<String>,
) -> Result<&'a mut Vec<Value>, String> {
    if map.get(key).is_none() {
        map.insert(key, Value::Array(Vec::new()));
        created.push(path.to_owned());
    }
    map.get_mut(key)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| format!("`{path}` is not an array"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMAND: &str = "'/owned dir/bin/threadspace-hook' hook --agent a.b.agent";
    const PREFIX: &str = "/owned dir/observer/";

    fn hook(event: &str) -> OwnedEntry {
        OwnedEntry::Hook {
            event: event.into(),
            matcher: String::new(),
            command: COMMAND.into(),
        }
    }

    fn plugin(path: &str) -> OwnedEntry {
        OwnedEntry::PluginDir { path: path.into() }
    }

    fn root(text: &str) -> Map {
        match Value::parse(text.as_bytes()).expect("parse") {
            Value::Object(map) => map,
            _ => panic!("object root"),
        }
    }

    fn plugin_dirs(map: &Map) -> Option<&str> {
        map.get("env")
            .and_then(|env| env.get(PLUGIN_DIRS))
            .and_then(Value::as_str)
    }

    #[test]
    fn plugin_dirs_append_once_preserving_existing_elements() {
        let ours = "/owned dir/observer/aaaa";
        let mut map = root(r#"{"env":{"CLAUDE_CODE_PLUGIN_DIRS":"/b:/a::/c","X":"1"}}"#);
        assert!(
            add(&mut map, &[plugin(ours)], PREFIX)
                .expect("add")
                .is_empty()
        );
        assert!(
            add(&mut map, &[plugin(ours)], PREFIX)
                .expect("add")
                .is_empty()
        );
        assert_eq!(
            plugin_dirs(&map),
            Some("/b:/a::/c:/owned dir/observer/aaaa")
        );

        // A newer owned copy replaces the stale one.
        let newer = "/owned dir/observer/bbbb";
        add(&mut map, &[plugin(newer)], PREFIX).expect("add");
        assert_eq!(
            plugin_dirs(&map),
            Some("/b:/a::/c:/owned dir/observer/bbbb")
        );

        let removal = remove(&mut map, &[plugin(newer)], &[]);
        assert_eq!(removal.removed.len(), 1);
        assert_eq!(plugin_dirs(&map), Some("/b:/a::/c"));
    }

    #[test]
    fn empty_plugin_dirs_value_is_restored_as_empty() {
        let ours = "/owned dir/observer/aaaa";
        let mut map = root(r#"{"env":{"CLAUDE_CODE_PLUGIN_DIRS":""}}"#);
        add(&mut map, &[plugin(ours)], PREFIX).expect("add");
        assert_eq!(plugin_dirs(&map), Some(ours));
        remove(&mut map, &[plugin(ours)], &[]);
        assert_eq!(plugin_dirs(&map), Some(""));
    }

    #[test]
    fn group_holding_a_foreign_hook_is_kept() {
        let mut map = Map::new();
        let created = add(&mut map, &[hook("Stop")], PREFIX).expect("add");
        assert_eq!(created, ["/hooks", "/hooks/Stop"]);
        // The owner adds their own hook into our group.
        let Some(Value::Array(groups)) = map
            .get_mut("hooks")
            .and_then(Value::as_object_mut)
            .and_then(|hooks| hooks.get_mut("Stop"))
        else {
            panic!("Stop groups");
        };
        let Some(Value::Array(list)) = groups[0].as_object_mut().and_then(|g| g.get_mut("hooks"))
        else {
            panic!("group hooks");
        };
        list.push(Value::parse(br#"{"type":"command","command":"theirs"}"#).expect("hook"));

        let removal = remove(&mut map, &[hook("Stop")], &created);
        assert_eq!(removal.removed.len(), 1);
        let printed =
            String::from_utf8(Value::Object(map).to_pretty().expect("print")).expect("utf-8");
        assert!(
            printed.contains("\"theirs\"") && !printed.contains(COMMAND),
            "{printed}"
        );
    }

    #[test]
    fn wrong_container_types_stop_the_edit() {
        for text in [
            r#"{"hooks":[]}"#,
            r#"{"hooks":{"Stop":{}}}"#,
            r#"{"env":"x"}"#,
            r#"{"env":{"CLAUDE_CODE_PLUGIN_DIRS":["/a"]}}"#,
        ] {
            let mut map = root(text);
            assert!(
                add(&mut map, &[hook("Stop"), plugin("/p")], PREFIX).is_err(),
                "{text}"
            );
        }
    }

    #[test]
    fn removal_reports_entries_no_longer_found() {
        let mut map = Map::new();
        let created = add(&mut map, &[hook("Stop"), hook("Notification")], PREFIX).expect("add");
        map.get_mut("hooks")
            .and_then(Value::as_object_mut)
            .expect("hooks")
            .remove("Notification");
        let removal = remove(&mut map, &[hook("Stop"), hook("Notification")], &created);
        assert_eq!(removal.removed, [hook("Stop")]);
        assert_eq!(removal.conflicts, [hook("Notification")]);
        assert!(map.is_empty(), "created containers are removed once empty");
    }
}
