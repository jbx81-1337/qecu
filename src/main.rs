mod qecu;
mod utils;
mod logging;
use crate::qecu::api;
use crate::qecu::channel::{ChannelBroker, ChannelConfig, ChannelTransport, ZmqTransport};
use crate::qecu::emulator::Emulator;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
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

    // Channel broker: validates/routes frames; transport tasks own the sockets.
    let channel_configs: Vec<ChannelConfig> = wf.channels.clone();
    let broker: Arc<Mutex<ChannelBroker>> =
        Arc::new(Mutex::new(ChannelBroker::new(channel_configs.clone())));
    for cfg in channel_configs {
        if cfg.transport != "zeromq" {
            eprintln!("qecu: channel '{}' has unsupported transport '{}'", cfg.name, cfg.transport);
            continue;
        }
        let endpoint = cfg
            .endpoint
            .clone()
            .unwrap_or_else(|| format!("ipc:///tmp/qecu-{}.sock", cfg.name));
        let broker = broker.clone();
        let name = cfg.name.clone();
        spawn(move || {
            let mut transport = match ZmqTransport::bind(&endpoint) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("qecu: channel '{name}' bind failed: {e}");
                    return;
                }
            };
            eprintln!("qecu: channel '{name}' listening on {endpoint}");
            loop {
                if let Ok(Some(msg)) = transport.recv(50) {
                    let replies = broker.lock().unwrap().process_inbound(msg);
                    for reply in replies {
                        if let Err(e) = transport.send(reply) {
                            eprintln!("qecu: channel '{name}' send failed: {e}");
                        }
                    }
                }
                let outbound = broker.lock().unwrap().drain_outbound();
                for msg in outbound {
                    if let Err(e) = transport.send(msg) {
                        eprintln!("qecu: channel '{name}' send failed: {e}");
                    }
                }
            }
        });
    }

    let emulator: Emulator<'static> = Emulator::new(wf, broker);
    let emustart = emulator.clone();

    spawn(move || {
        emustart.run();
    });

    api::bootstrap(String::from("127.0.0.1:3000"), emulator.clone()).await;
}
