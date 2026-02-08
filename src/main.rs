use ron_lsp::Backend;
use tower_lsp::{LspService, Server};

#[tokio::main]
async fn main() {
    #[cfg(feature = "cli")]
    {
        use clap::Parser;
        let cli = ron_lsp::Cli::parse();

        match cli.command {
            Some(ron_lsp::Commands::Check { directory }) => {
                ron_lsp::run_check(directory).await;
                return;
            }
            None => {
                // Fall through to LSP server
            }
        }
    }

    // Run as LSP server
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
