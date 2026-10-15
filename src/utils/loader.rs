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
    fn load_raw_cs(path: &str, address: u64) -> CodeSection {
        let data = std::fs::read(path).unwrap_or_else(|error| {
            panic!("[qecu::loader] Could not read raw file {path}: {error}")
        });
        CodeSection {
            address,
            size: data.len(),
            data,
        }
    }

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
            if _section.sh_addr == 0 || _section.sh_type == elf::abi::SHT_NOBITS {
                continue;
            }
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

    fn load_elf_entry(path: &String) -> u64 {
        let file_data = std::fs::read(std::path::PathBuf::from(path)).expect("[Could not read file.");
        let file = ElfBytes::<AnyEndian>::minimal_parse(file_data.as_slice()).unwrap();
        file.ehdr.e_entry
    }

    pub fn load_entry(format: &str, path: &String) -> u64 {
        match format {
            "elf" => Loader::load_elf_entry(path),
            "raw" => 0,
            _ => panic!("[qecu::loader] Unsupported Format.\n")
        }
    }

    pub fn load_code_sections(format: &str, path: &String) -> Vec<CodeSection> {
        let ret = match format {
            "elf"   => Loader::load_elf_cs(path),
            "raw"   => vec![Loader::load_raw_cs(path, 0)],
            // "ihex"  => Loader::load_ihex(path),
            // "srec"  => Loader::load_srec(path),
            _       => panic!("[qecu::loader] Unsupported Format.\n")
        };
        return ret;
    }

    pub fn load_raw_code_section(path: &str, address: u64) -> CodeSection {
        Loader::load_raw_cs(path, address)
    }

    pub fn load_functions(format: &str, path: &String) -> HashMap<String, u64> {
        let ret: HashMap<String, u64> = HashMap::new();
        return ret;
    }
}

#[cfg(test)]
mod tests {
    use super::Loader;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn loads_raw_file_as_code_section_at_requested_address() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("qecu-raw-{unique}.bin"));
        std::fs::write(&path, [0x00, 0x01, 0xfe, 0xff]).unwrap();

        let section = Loader::load_raw_code_section(path.to_str().unwrap(), 0x2000);

        assert_eq!(section.address, 0x2000);
        assert_eq!(section.size, 4);
        assert_eq!(section.data, [0x00, 0x01, 0xfe, 0xff]);

        let path_string = path.to_string_lossy().into_owned();
        let sections = Loader::load_code_sections("raw", &path_string);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].address, 0);
        assert_eq!(sections[0].data, [0x00, 0x01, 0xfe, 0xff]);
        assert_eq!(Loader::load_entry("raw", &path_string), 0);

        std::fs::remove_file(path).unwrap();
    }
}