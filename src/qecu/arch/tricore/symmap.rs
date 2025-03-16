
pub trait SymMap {
    fn get_symbol(&self, symbol: String) -> u32;
}