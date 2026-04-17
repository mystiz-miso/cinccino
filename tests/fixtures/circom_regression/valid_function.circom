pragma circom 2.0.0;

function nbits(a) {
    var n = 1;
    var r = 0;
    while (n - 1 < a) {
        r++;
        n = n + n;
    }
    return r;
}

template T(n) {
    signal input x;
    signal output y;
    y <== x;
}
