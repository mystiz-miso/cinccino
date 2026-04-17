pragma circom 2.0.0;

template Adder(n) {
    signal input a;
    signal input b;
    signal output c;
    c <== a + b;
}
