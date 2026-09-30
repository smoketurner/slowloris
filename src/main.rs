//! slowloris — a small, statically-linkable slow-HTTP resilience tester.
//!
//! Slow-HTTP techniques (Slowloris slow-headers, slow-POST, slow-read) hold a
//! server's connections open using almost no bandwidth, exercising how it
//! behaves under connection-pool exhaustion. Use it only against servers you
//! own or are explicitly authorized to test.

mod agents;
mod cli;
mod engine;
mod rng;
mod stream;

use std::process::ExitCode;

use clap::Parser;
use mimalloc::MiMalloc;

use cli::{Args, Target};
use engine::Engine;

/// Use mimalloc (built in hardened "secure" mode) as the global allocator.
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() -> ExitCode {
    let args = Args::parse();

    let target = match Target::parse(&args.url, args.path.as_deref()) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };

    let mut engine = match Engine::new(&args, target) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };

    match engine.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("\nerror: {e}");
            ExitCode::FAILURE
        }
    }
}
