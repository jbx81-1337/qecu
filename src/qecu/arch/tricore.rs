use symmap::SymMap;
use tc375::TricoreCpuTc375;

mod tc375;
mod symmap;

pub fn get_cpu_symbol(cpu: String, symbol: String) -> u32 {
    match cpu.as_str() {
        "tc375" => Box::new(TricoreCpuTc375 { }).get_symbol(symbol),
        _ => 0
    }
}