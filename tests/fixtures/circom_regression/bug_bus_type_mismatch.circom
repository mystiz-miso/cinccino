pragma circom 2.2.0;

bus A() {
    signal v;
}

bus B() {
    signal v;
}

template T() {
    signal input A() a;
    signal output B() b;
    b <== a;
}
