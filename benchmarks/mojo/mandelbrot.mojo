# UNVERIFIED reference port (no Mojo toolchain on CI host). See README.md.
fn mandel_iter(cr: Float64, ci: Float64, max_iter: Int) -> Int:
    var zr: Float64 = 0.0
    var zi: Float64 = 0.0
    for i in range(max_iter):
        var zr2 = zr * zr
        var zi2 = zi * zi
        if zr2 + zi2 > 4.0:
            return i
        var nzr = zr2 - zi2 + cr
        zi = 2.0 * zr * zi + ci
        zr = nzr
    return max_iter

fn main():
    var width = 1000
    var height = 1000
    var max_iter = 1000
    var checksum: Int = 0
    for y in range(height):
        for x in range(width):
            var cr = -2.0 + 3.0 * Float64(x) / Float64(width)
            var ci = -1.5 + 3.0 * Float64(y) / Float64(height)
            checksum += mandel_iter(cr, ci, max_iter)
    print("mandelbrot checksum =", checksum)
