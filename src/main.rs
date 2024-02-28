mod qecu;
mod utils;
use std::fs;
use crate::qecu::emulator::Emulator;

fn main() {
    let config: String = fs::read_to_string("./config.yml").unwrap();
    let wf = utils::workflow::Workflow::new(config);
    print!("{}\n", wf.project);
    let mut emulator: Emulator<'_> = Emulator::new(wf);
    emulator.run();
}