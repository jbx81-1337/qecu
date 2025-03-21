use std::collections::HashMap;

use elf::endian::AnyEndian;
use elf::ElfBytes;

pub struct CodeSection {
    pub address: u64,
    pub size: usize,
    pub data: Vec<u8>,
}

pub struct Loader {}
impl Loader {
    fn load_elf_cs(path: &String) -> Vec<CodeSection> {
        let path = std::path::PathBuf::from(path);
        let file_data = std::fs::read(path).expect("[Could not read file.");
        let slice = file_data.as_slice();
        let file = ElfBytes::<AnyEndian>::minimal_parse(slice).unwrap();
        let _segments = file.segments().unwrap();
        let mut ret: Vec<CodeSection> = Vec::new();
        
        print!("[qecu::loader] Loading segments.\n");
        for _segment in _segments.iter() {
            let _data: Vec<u8> = file.segment_data(&_segment).unwrap().to_vec();
            let code_section: CodeSection = CodeSection {
                address: _segment.p_vaddr,
                size: _data.len(),
                data: _data
            };
            ret.push(code_section);
        }

        print!("[qecu::loader] Loading sections.\n");
        let _sections = file.section_headers().unwrap();
        for _section in _sections.iter() {
            let _data = file.section_data(&_section).unwrap().0;
            let code_section: CodeSection = CodeSection {
                address: _section.sh_addr,
                size: _data.len(),
                data: _data.to_vec()
            };
            ret.push(code_section);
        }
        return ret;
    }

    fn load_elf_functions(path: &String) -> HashMap<String, u64> {
        let ret: HashMap<String, u64> = HashMap::new();

        let path = std::path::PathBuf::from(path);
        let file_data = std::fs::read(path).expect("[Could not read file.");
        let slice = file_data.as_slice();
        let file = ElfBytes::<AnyEndian>::minimal_parse(slice).unwrap();

        let sym_table = file.symbol_table().unwrap();
        for _symbol in sym_table.iter() {}

        return ret;
    }

    pub fn load_code_sections(format: &str, path: &String) -> Vec<CodeSection> {
        let ret = match format {
            "elf"   => Loader::load_elf_cs(path),
            // "ihex"  => Loader::load_ihex(path),
            // "srec"  => Loader::load_srec(path),
            _       => panic!("[qecu::loader] Unsupported Format.\n")
        };
        return ret;
    }

    pub fn load_functions(format: &str, path: &String) -> HashMap<String, u64> {
        let ret: HashMap<String, u64> = HashMap::new();
        return ret;
    }
}