//! An example demonstrating how to parse Rust source files and serve an LSP.
//!
//! This example uses the `analyze` feature to parse type definitions from a Rust
//! source file, then serves an LSP server for RON configuration files.
//!
//! Run with: `cargo run --example analyze_lsp --features "analyze,lsp"`

use roniker::RustAnalyzer;
use std::path::Path;

fn main() {
    // Create a RustAnalyzer with the root configuration type path
    let mut analyzer = RustAnalyzer::with_root_type("AppConfig");

    // Parse types from a Rust source file
    let config_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/data/config_types.rs");

    analyzer
        .add_file(&config_path)
        .expect("Failed to parse config_types.rs");

    // Verify the types were parsed
    println!("Parsed types:");
    for type_info in analyzer.get_all_types() {
        println!("  - {}: {:?}", type_info.name, type_info.docs);
    }

    println!("\nStarting LSP server on stdin/stdout...");
    println!("Configure your editor to use this as a language server for .ron files.");

    // Start the LSP server
    tokio::runtime::Runtime::new()
        .expect("Failed to create Tokio runtime")
        .block_on(roniker::serve(analyzer, true));
}
