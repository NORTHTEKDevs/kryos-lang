# UNVERIFIED reference port (no Mojo toolchain on CI host). See README.md.
from memory import UnsafePointer

fn main():
    var n = 512
    var a = UnsafePointer[Int64].alloc(n * n)
    var b = UnsafePointer[Int64].alloc(n * n)
    var c = UnsafePointer[Int64].alloc(n * n)
    for i in range(n * n):
        a[i] = (Int64(i) * 3) % 7
        b[i] = (Int64(i) * 5) % 11
        c[i] = 0
    for row in range(n):
        for col in range(n):
            var s: Int64 = 0
            for k in range(n):
                s += a[row * n + k] * b[k * n + col]
            c[row * n + col] = s
    var checksum: Int64 = 0
    for i in range(n * n):
        checksum += c[i]
    print("matmul(512) checksum =", checksum)
    a.free(); b.free(); c.free()
