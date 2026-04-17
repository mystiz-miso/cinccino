pragma circom 2.0.0;

template ArraySum(n) {
    signal input a[n];
    signal output sum;
    var s = 0;
    for (var i = 0; i < n; i++) {
        s = s + a[i];
    }
    sum <== s;
}
