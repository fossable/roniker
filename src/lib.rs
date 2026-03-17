//! # roniker
//!
//! A library for creating custom Language Server Protocol (LSP) servers for applications
//! configured with [RON](https://github.com/ron-rs/ron) (Rusty Object Notation).
//!
//! ## Features
//!
//! - `analyze` - Parse Rust source files to extract type definitions
//! - `lsp` - Serve an LSP server with hover, completions, diagnostics, and more
//!
//! ## Usage
//!
//! ```
//! use roniker::{RustAnalyzer, TypeInfo, TypeKind, FieldInfo};
//!
//! // Create an analyzer with the root configuration type
//! let mut analyzer = RustAnalyzer::new("crate::config::Config");
//!
//! // Register types directly
//! analyzer.add_type(TypeInfo {
//!     name: "crate::config::Config".to_string(),
//!     kind: TypeKind::Struct(vec![
//!         FieldInfo {
//!             name: "debug".to_string(),
//!             type_name: "bool".to_string(),
//!             docs: Some("Enable debug mode".to_string()),
//!             line: None,
//!             column: None,
//!             has_default: false,
//!         },
//!     ]),
//!     docs: Some("Application configuration".to_string()),
//!     source_file: None,
//!     line: None,
//!     column: None,
//!     has_default: false,
//! });
//!
//! // Query type information
//! let config = analyzer.root_type_info();
//! assert_eq!(config.name, "crate::config::Config");
//! ```

pub mod rust_analyzer;

pub use rust_analyzer::{EnumVariant, FieldInfo, RustAnalyzer, TypeInfo, TypeKind};

#[cfg(feature = "lsp")]
mod lsp;

#[cfg(feature = "lsp")]
pub use lsp::serve;
