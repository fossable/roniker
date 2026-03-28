//! A simple example demonstrating how to create and serve a custom RON LSP.
//!
//! This example creates type definitions directly (without parsing Rust source files)
//! and serves an LSP server that provides IDE features for RON configuration files.
//!
//! Run with: `cargo run --example simple_lsp --features "lsp"`

use roniker::{EnumVariant, FieldInfo, RustAnalyzer, TypeInfo, TypeKind};

fn main() {
    // Create a RustAnalyzer with the root configuration type
    let mut analyzer = RustAnalyzer::with_root_type("Config");

    // Register the root Config struct
    analyzer.add_type(TypeInfo {
        name: "Config".to_string(),
        kind: TypeKind::Struct(vec![
            FieldInfo {
                name: "debug".to_string(),
                type_name: "bool".to_string(),
                docs: Some("Enable debug mode for verbose logging".to_string()),
                line: None,
                column: None,
                has_default: true,
            },
            FieldInfo {
                name: "port".to_string(),
                type_name: "u16".to_string(),
                docs: Some("The port number to listen on".to_string()),
                line: None,
                column: None,
                has_default: false,
            },
            FieldInfo {
                name: "database".to_string(),
                type_name: "DatabaseConfig".to_string(),
                docs: Some("Database connection settings".to_string()),
                line: None,
                column: None,
                has_default: false,
            },
            FieldInfo {
                name: "log_level".to_string(),
                type_name: "LogLevel".to_string(),
                docs: Some("The logging verbosity level".to_string()),
                line: None,
                column: None,
                has_default: true,
            },
        ]),
        docs: Some("Main application configuration".to_string()),
        source_file: None,
        line: None,
        column: None,
        has_default: false,
    });

    // Register the nested DatabaseConfig struct
    analyzer.add_type(TypeInfo {
        name: "DatabaseConfig".to_string(),
        kind: TypeKind::Struct(vec![
            FieldInfo {
                name: "host".to_string(),
                type_name: "String".to_string(),
                docs: Some("Database server hostname".to_string()),
                line: None,
                column: None,
                has_default: false,
            },
            FieldInfo {
                name: "port".to_string(),
                type_name: "u16".to_string(),
                docs: Some("Database server port".to_string()),
                line: None,
                column: None,
                has_default: true,
            },
            FieldInfo {
                name: "username".to_string(),
                type_name: "String".to_string(),
                docs: Some("Database username".to_string()),
                line: None,
                column: None,
                has_default: false,
            },
            FieldInfo {
                name: "max_connections".to_string(),
                type_name: "Option<u32>".to_string(),
                docs: Some("Maximum number of connections in the pool".to_string()),
                line: None,
                column: None,
                has_default: false,
            },
        ]),
        docs: Some("Database connection configuration".to_string()),
        source_file: None,
        line: None,
        column: None,
        has_default: false,
    });

    // Register the LogLevel enum
    analyzer.add_type(TypeInfo {
        name: "LogLevel".to_string(),
        kind: TypeKind::Enum(vec![
            EnumVariant {
                name: "Error".to_string(),
                fields: vec![],
                docs: Some("Only log errors".to_string()),
                line: None,
                column: None,
            },
            EnumVariant {
                name: "Warn".to_string(),
                fields: vec![],
                docs: Some("Log warnings and errors".to_string()),
                line: None,
                column: None,
            },
            EnumVariant {
                name: "Info".to_string(),
                fields: vec![],
                docs: Some("Log info, warnings, and errors".to_string()),
                line: None,
                column: None,
            },
            EnumVariant {
                name: "Debug".to_string(),
                fields: vec![],
                docs: Some("Log everything including debug messages".to_string()),
                line: None,
                column: None,
            },
        ]),
        docs: Some("Logging verbosity level".to_string()),
        source_file: None,
        line: None,
        column: None,
        has_default: false,
    });

    // Start the LSP server (reads from stdin, writes to stdout)
    tokio::runtime::Runtime::new()
        .expect("Failed to create Tokio runtime")
        .block_on(roniker::serve(analyzer, true));
}
