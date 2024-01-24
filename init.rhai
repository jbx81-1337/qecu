let intercept = new_interceptor(emulator);

let mema = intercept.read_memory(0xa0300200, 8);
print(mema);
let buf = blob(8, 5);
buf.write_be(0, 8, 0xab_cd_ef_12_34_56_78_90);
let out = intercept.write_memory(0xa0300200, buf);
let mem = intercept.read_memory(0xa0300200, 8);
print(mem);