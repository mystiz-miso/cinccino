pragma circom 2.0.0;

template Param(n) {
    signal input x;
    signal output y;
    y <== x;
}

template Main() {
    component c = Param(1, 2);
}
