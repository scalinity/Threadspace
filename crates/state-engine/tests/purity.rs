//! The reducer's purity (SPEC §2.3, §5.5) as a checked property: its modules
//! reference no clock, randomness, filesystem, process, environment, thread,
//! network or global mutable state. Allocation of random identities lives
//! only in `ids.rs` (`RandomAllocator`), which admission uses before
//! reduction and the reducer never calls.

const REDUCER_MODULES: &[(&str, &str)] = &[
    ("engine.rs", include_str!("../src/engine.rs")),
    ("reduce.rs", include_str!("../src/reduce.rs")),
    ("causal.rs", include_str!("../src/causal.rs")),
    ("semantic.rs", include_str!("../src/semantic.rs")),
    ("hash.rs", include_str!("../src/hash.rs")),
    ("keys.rs", include_str!("../src/keys.rs")),
    ("profiles.rs", include_str!("../src/profiles.rs")),
    ("command.rs", include_str!("../src/command.rs")),
    ("validate.rs", include_str!("../src/validate.rs")),
    ("normalize.rs", include_str!("../src/normalize.rs")),
    ("synthetic.rs", include_str!("../src/synthetic.rs")),
];

const FORBIDDEN: &[&str] = &[
    "SystemTime",
    "Instant",
    "std::time",
    "chrono",
    "rand::",
    "thread_rng",
    "new_v4",
    "getrandom",
    "arc4random",
    "std::fs",
    "File::",
    "std::process",
    "std::env",
    "std::thread",
    "std::net",
    "static mut",
    "thread_local",
    "OnceLock",
    "lazy_static",
    "Mutex",
    "RwLock",
    "Atomic",
    "libc::",
];

#[test]
fn reducer_modules_read_no_ambient_state() {
    let mut found = Vec::new();
    for (name, source) in REDUCER_MODULES {
        for (line_number, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            for token in FORBIDDEN {
                if code.contains(token) {
                    found.push(format!("{name}:{}: {token}", line_number + 1));
                }
            }
        }
    }
    assert!(found.is_empty(), "ambient state in the reducer: {found:#?}");
}

#[test]
fn only_the_admission_allocator_draws_randomness() {
    let ids = include_str!("../src/ids.rs");
    let random_uses: Vec<&str> = ids.lines().filter(|l| l.contains("new_v4")).collect();
    assert_eq!(random_uses.len(), 1, "one random draw, in RandomAllocator: {random_uses:?}");
    assert!(ids.contains("impl Allocator for RandomAllocator"));
}
