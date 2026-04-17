template Adder(n) {
    signal input a;
    signal output b;
    b <== a;
}

template Main() {
    component c = Adder(4);
}
