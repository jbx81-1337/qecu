fn load(model) {
    import model as cpu;
    print(`> Loading Tricore ${model} CPU`);
    return #{
        mm: cpu::memory_map()
    };
}