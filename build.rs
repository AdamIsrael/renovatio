//! Computes `RENOVATIO_ABI`, a fingerprint of everything the plugin ABI depends on.
//!
//! Plugins and the host exchange Rust types across a `dlopen` boundary, which
//! is only sound when both were built by the same compiler from the same
//! plugin interface. The host refuses plugins whose fingerprint differs.
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-env-changed=RUSTC");

    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let rustc_version = Command::new(rustc)
        .arg("-V")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|version| version.trim().to_string())
        .unwrap_or_else(|| "unknown rustc".to_string());

    let lib = std::fs::read("src/lib.rs").expect("failed to read src/lib.rs");

    println!(
        "cargo:rustc-env=RENOVATIO_ABI=renovatio {}; {}; interface {:016x}",
        std::env::var("CARGO_PKG_VERSION").unwrap(),
        rustc_version,
        fnv1a(&lib)
    );
}

/// FNV-1a: a small, stable hash, so the fingerprint doesn't depend on std's hasher.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}
