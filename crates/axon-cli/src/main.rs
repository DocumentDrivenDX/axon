//! Axon command-line interface.
//!
//! Entry point for the `axon` binary.

fn main() {
    tracing_subscriber::fmt::init();
    println!("axon v{}", env!("CARGO_PKG_VERSION"));
}
