mod qecu;
mod utils;
use crate::qecu::api;
use crate::qecu::emulator::Emulator;
use std::fs;
use std::thread::spawn;

#[tokio::main]
async fn main() {
    let config: String = fs::read_to_string("./config.yml").unwrap();
    let wf = utils::workflow::Workflow::new(config);
    println!("{}", wf.project);

    let emulator: Emulator<'static> = Emulator::new(wf);
    let emustart = emulator.clone();

    spawn(move || {
        emustart.run();
    });

    api::bootstrap(String::from("127.0.0.1:3000"), emulator.clone()).await;
}
