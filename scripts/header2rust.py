# Simple Helper tool to generate a memory_map Rhai file from a .h
import sys
import re

re_groups = r"#define ([A-Z0-9_]+).*(0x[A-F0-9]{8})"
const_def = ''
map_def = ''

source_placeholder = '''
use crate::qecu::arch::tricore::symmap::SymMap;

{const_def}

pub struct TricoreCpuTc375;
impl SymMap for TricoreCpuTc375 {
    fn get_symbol(&self, symbol: String) -> u32 {
        match symbol.as_str() {
{map_def}
        }
    }
}
'''

if __name__ == '__main__':
    for line in sys.stdin:
        matches = re.findall(re_groups, line)
        matches = matches[0]
        const_def += f"const {matches[0]}: u32 = {matches[1]};\n"
        t = 4
        if len(matches[0]) > 11:
            t -= 1
        map_def += f"\t\t\t\"{matches[0]}\"{ '\t' * t } => {matches[0]},\n"
    source_placeholder = source_placeholder.replace("{const_def}", const_def)
    source_placeholder = source_placeholder.replace("{map_def}", map_def)
    print(source_placeholder)
