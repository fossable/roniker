pub mod rust_analyzer;

pub use rust_analyzer::{EnumVariant, FieldInfo, RustAnalyzer, TypeInfo, TypeKind};

#[cfg(feature = "lsp")]
mod lsp;

#[cfg(feature = "lsp")]
pub use lsp::serve;
