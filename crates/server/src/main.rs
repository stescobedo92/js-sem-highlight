//! Binario `js-sem-highlight`: arranca el servidor LSP por stdio.

use std::env;
use std::process::ExitCode;

use js_sem_lsp::Backend;
use tower_lsp::{LspService, Server};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    if handle_cli_flags() {
        return ExitCode::SUCCESS;
    }

    install_tracing();
    install_panic_hook();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
    ExitCode::SUCCESS
}

fn handle_cli_flags() -> bool {
    let mut args = env::args().skip(1);
    let Some(first) = args.next() else {
        return false;
    };
    match first.as_str() {
        "--version" | "-V" => {
            println!("js-sem-highlight {}", env!("CARGO_PKG_VERSION"));
            true
        }
        "--help" | "-h" => {
            println!(
                "js-sem-highlight {}\n\nLSP server providing semantic tokens and visual lint hints \
                 for JavaScript/TypeScript/JSX/TSX.\n\nUSAGE: js-sem-highlight\n\nThe server \
                 communicates via stdio and is intended to be launched by an LSP client.",
                env!("CARGO_PKG_VERSION")
            );
            true
        }
        _ => false,
    }
}

fn install_tracing() {
    let filter = EnvFilter::try_from_env("JS_SEM_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .json()
        .init();
}

fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map_or_else(|| "<unknown>".to_string(), ToString::to_string);
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<no message>".into());
        tracing::error!(location = %location, message = %message, "panic in worker thread");
        prev(info);
    }));
}
