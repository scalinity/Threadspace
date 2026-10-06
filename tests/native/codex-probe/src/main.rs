//! M0C Codex platform probe (read-only). Never shipped.

fn main() -> std::process::ExitCode {
    eprintln!("usage: threadspace-codex-probe <command>");
    std::process::ExitCode::from(64)
}
