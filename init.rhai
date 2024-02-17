fn code_hook(address, size) {
    let disas = Interceptor.disas(address, size);
    print(`0x${address.to_hex()}:> ${disas}`);
    return 0;
}
Interceptor.add_code_hook("code_hook");