pragma circom 2.0.0;

template Adder() {
    signal input a;
    signal input b;
    signal output c;
    c <== a + b;
}

template Main() {
    signal input x;
    signal input y;
    signal output z;
    component adder = Adder();
    adder.a <== x;
    adder.b <== y;
    z <== adder.c;
}
