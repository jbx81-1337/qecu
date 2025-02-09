import "rhai/tricore/tricore" as tricore;

// Memory Maps symbol on Rhai side are slow af
// Need to be moved diretly in Rust.
// let cpu = "tc375";
// print(`> Loading ${cpu} Memory Map`);
// let ecu = tricore::load("tc375");
let ecu = #{};
print("> Mempry Map loaded");

print("> Interceptor initialization.");
/*
 * IfxCpu_waitEvent: Hook, just skip to ret
 */
Interceptor.add_hook("CODE", 0x800019de, 4, |Interceptor, addr, size| {
    print("> IfxCpu_waitEvent. Skipping");
    Interceptor.write_register("D2", 0);
    Interceptor.set_pc(0x80001a58);
    return 0;
});

Interceptor.add_hook("CODE", 0x80000200, 4, |Interceptor, addr, size| {
    // print("> WaitTime, Waiting 10ms...");
    Interceptor.sleep(10);
    Interceptor.set_pc(0x800002e2);
    
    return 0;
});

Interceptor.add_hook("CODE", 0x80003f4c, 4, |Interceptor, addr, size| {
    print("> SoftwareStartup finished, core0_start...");                    // prints "hello! from module!"   
    return 0;
});

Interceptor.add_hook_with_data("CODE", 0x80003eac, 0x10000000, |Interceptor, addr, size, ecu| {
    print(`${Interceptor.disas(addr, size)}`);
    return 0;
}, ecu);

Interceptor.on_event("event-test", |Interceptor, event_type, msg| {
    print(event_type);
    print(msg);
    return 0;
});
