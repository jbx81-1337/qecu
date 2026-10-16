mod qecu;
mod utils;
mod logging;
use crate::qecu::api;
use crate::qecu::emulator::Emulator;
use std::fs;
use std::path::Path;
use std::thread::spawn;

fn print_usage() {
    println!("Usage: qecu [--config <path>] [--debug]");
    println!("  --config <path>  Path to the emulator configuration file (default: ./config.yml).");
    println!("  --debug           Print QECU debug logs ([emulator::sysreg], [emulator::mem], ...).");
}

#[tokio::main]
async fn main() {
    // Parse command-line arguments.
    let args: Vec<String> = std::env::args().collect();
    let mut config_path = String::from("./config.yml");
    let mut debug = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--config" | "-c" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("qecu: --config requires a path");
                    std::process::exit(2);
                }
                config_path = args[i].clone();
            }
            "--debug" | "-d" => debug = true,
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other => {
                eprintln!("qecu: unknown argument '{other}'");
                print_usage();
                std::process::exit(2);
            }
        }
        i += 1;
    }
    logging::set_debug(debug);

    let config: String = fs::read_to_string(&config_path).unwrap_or_else(|e| {
        eprintln!("qecu: cannot read config '{}': {e}", config_path);
        std::process::exit(1);
    });

    // Resolve the config's relative paths (content, init_script, ...) against
    // the config file's directory, so `--config` works from any cwd.
    if let Some(parent) = Path::new(&config_path).parent().filter(|p| !p.as_os_str().is_empty()) {
        if let Err(e) = std::env::set_current_dir(parent) {
            eprintln!("qecu: cannot enter config directory '{}': {e}", parent.display());
            std::process::exit(1);
        }
    }

    let wf = utils::workflow::Workflow::new(config);
    println!("{}", wf.project);

    let emulator: Emulator<'static> = Emulator::new(wf);
    let emustart = emulator.clone();

    spawn(move || {
        emustart.run();
    });

    api::bootstrap(String::from("127.0.0.1:3000"), emulator.clone()).await;
}
