#[allow(dead_code)]
#[path = "/workspace/scratch/c8272833f682/Threadspace-review/tests/synthetic/src/bin/m1/evidence.rs"]
mod evidence;
#[path = "/workspace/scratch/c8272833f682/Threadspace-review/tests/synthetic/src/bin/m1/permutations.rs"]
mod permutations;
fn main() {
    let output = std::env::args_os().nth(1).expect("explicit qualification output");
    let result = permutations::run_all(std::path::Path::new(&output), 20_000).expect("campaign");
    println!("{}", serde_json::to_string_pretty(&result).expect("summary"));
    assert_eq!(result["pass"], true, "unchanged generator and oracle must pass");
}
