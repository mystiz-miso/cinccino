pragma circom 2.0.0;

template T() {
    signal input a;
    signal output b;
    a <== 1;
    b <== a;
}
