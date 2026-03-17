// Example configuration types for the analyze_lsp example.
//
// This file demonstrates the Rust structs that roniker can parse
// to automatically generate LSP support for RON configuration files.

use serde::{Deserialize, Serialize};

/// Main application configuration.
///
/// This is the root configuration type that will be parsed by the LSP.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppConfig {
    /// Enable debug mode for verbose logging.
    #[serde(default)]
    pub debug: bool,

    /// The port number to listen on.
    pub port: u16,

    /// Server configuration.
    pub server: ServerConfig,

    /// Optional feature flags.
    #[serde(default)]
    pub features: Vec<String>,
}

/// Server configuration options.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerConfig {
    /// The hostname to bind to.
    pub host: String,

    /// Maximum number of concurrent connections.
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,

    /// Connection timeout in seconds.
    pub timeout: Option<u64>,

    /// The server mode.
    #[serde(default)]
    pub mode: ServerMode,
}

fn default_max_connections() -> u32 {
    100
}

/// Server operating mode.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum ServerMode {
    /// Development mode with hot reloading.
    #[default]
    Development,
    /// Staging mode for testing.
    Staging,
    /// Production mode with optimizations.
    Production,
}
