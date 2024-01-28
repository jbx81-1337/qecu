fn code_hook(address, size) {
    print(`Address: ${address.to_hex()}`);
    print(Interceptor.read_register("A0"));
    return 0;
}
Interceptor.add_code_hook("code_hook");