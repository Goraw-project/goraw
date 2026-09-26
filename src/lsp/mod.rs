pub mod analysis;
pub mod protocol;
pub mod server;

pub use server::LspServer;

pub fn run_lsp_server() -> std::io::Result<()> {
    let mut server = LspServer::new();
    server.run()
}
