#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `devhq --read-copy` reads one local copy for a window or a gate that started it: no window,
    // a request on stdin and the reading on stdout, in a process that cannot start programs.
    if std::env::args().nth(1).as_deref() == Some("--read-copy") {
        std::process::exit(devhq_lib::local_copies::read_copy_main());
    }
    // `devhq --mcp` is the agent gate: no window, only MCP over stdin and stdout.
    if std::env::args().skip(1).any(|arg| arg == "--mcp") {
        std::process::exit(devhq_lib::gate::main(std::env::args().skip(1)));
    }
    devhq_lib::run();
}
