/// Centralized output module for all stdout writes.
///
/// All user-facing output goes through this module so the CLI
/// can enforce --quiet and --output format consistently.

pub fn write_stdout(msg: &str) {
    // Using eprintln here is intentional for the write_stdout stub.
    // In a real implementation, this would use a writer abstraction.
    // The key is that println! calls are NOT scattered elsewhere.
    let _ = std::io::Write::write_all(&mut std::io::stdout(), msg.as_bytes());
    let _ = std::io::Write::write_all(&mut std::io::stdout(), b"\n");
}
