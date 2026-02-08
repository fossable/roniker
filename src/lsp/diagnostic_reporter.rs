use tower_lsp::lsp_types::{Diagnostic as LspDiagnostic, DiagnosticSeverity};

/// A diagnostic that is independent of the output format (LSP or ariadne)
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub line: u32,
    pub col_start: u32,
    pub col_end: u32,
    pub severity: Severity,
    pub message: String,
}

impl PartialEq for Diagnostic {
    fn eq(&self, other: &Self) -> bool {
        self.line == other.line && self.col_start == other.col_start
    }
}

impl Eq for Diagnostic {}

impl PartialOrd for Diagnostic {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Diagnostic {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Sort by line first, then by column
        match self.line.cmp(&other.line) {
            std::cmp::Ordering::Equal => self.col_start.cmp(&other.col_start),
            other => other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    #[allow(dead_code)]
    Info,
}

impl Diagnostic {
    #[allow(dead_code)]
    pub fn error(line: u32, col_start: u32, col_end: u32, message: String) -> Self {
        Self {
            line,
            col_start,
            col_end,
            severity: Severity::Error,
            message,
        }
    }

    #[allow(dead_code)]
    pub fn warning(line: u32, col_start: u32, col_end: u32, message: String) -> Self {
        Self {
            line,
            col_start,
            col_end,
            severity: Severity::Warning,
            message,
        }
    }

    /// Convert to LSP diagnostic
    #[allow(dead_code)]
    pub fn to_lsp(&self) -> LspDiagnostic {
        use tower_lsp::lsp_types::{Position, Range};

        LspDiagnostic {
            range: Range::new(
                Position::new(self.line, self.col_start),
                Position::new(self.line, self.col_end),
            ),
            severity: Some(match self.severity {
                Severity::Error => DiagnosticSeverity::ERROR,
                Severity::Warning => DiagnosticSeverity::WARNING,
                Severity::Info => DiagnosticSeverity::INFORMATION,
            }),
            message: self.message.clone(),
            ..Default::default()
        }
    }
}
