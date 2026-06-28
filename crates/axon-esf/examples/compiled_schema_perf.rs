use std::hint::black_box;
use std::time::Instant;

use axon_esf::CompiledSchema;
use serde_json::json;

fn main() {
    const WARMUP: usize = 10_000;
    const ITERATIONS: usize = 1_000_000;

    let schema = json!({
        "type": "object",
        "required": ["id", "status", "created_at"],
        "properties": {
            "id": {"type": "string", "format": "uuid"},
            "status": {"type": "string", "enum": ["ready", "running", "done"]},
            "created_at": {"type": "string", "format": "date-time"},
            "priority": {"type": "integer"}
        }
    });
    let record = json!({
        "id": "018f1f0a-8d2a-7b30-a4ef-0a60b00f9489",
        "status": "ready",
        "created_at": "2026-06-28T16:00:00Z",
        "priority": 3
    });

    let compiled = CompiledSchema::compile(black_box(&schema)).expect("schema should compile");

    for _ in 0..WARMUP {
        compiled
            .validate(black_box(&record))
            .expect("warmup validation should pass");
    }

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        compiled
            .validate(black_box(&record))
            .expect("validation should pass");
    }
    let elapsed = start.elapsed();
    let avg_ns = elapsed.as_nanos() / ITERATIONS as u128;

    println!("compiled schema validation average: {avg_ns} ns");

    if !cfg!(debug_assertions) && avg_ns >= 1_000 {
        eprintln!("compiled schema validation average must be < 1000 ns in release");
        std::process::exit(1);
    }
}
