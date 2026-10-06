#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `devhq --mcp` is the agent gate: no window, only MCP over stdin and stdout.
    if std::env::args().skip(1).any(|arg| arg == "--mcp") {
        std::process::exit(devhq_lib::gate::main(std::env::args().skip(1)));
    }
    devhq_lib::run();
}
