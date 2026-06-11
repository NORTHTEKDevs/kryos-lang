# Benchmark: Mandelbrot set — mirrors c/mandelbrot.c exactly (1000x1000, fp).
def mandel_iter(cr, ci, max_iter):
    zr = 0.0
    zi = 0.0
    for i in range(max_iter):
        zr2 = zr * zr
        zi2 = zi * zi
        if zr2 + zi2 > 4.0:
            return i
        new_zr = zr2 - zi2 + cr
        new_zi = 2.0 * zr * zi + ci
        zr = new_zr
        zi = new_zi
    return max_iter

width, height, max_iter = 1000, 1000, 1000
checksum = 0
for y in range(height):
    for x in range(width):
        cr = -2.0 + 3.0 * x / width
        ci = -1.5 + 3.0 * y / height
        checksum += mandel_iter(cr, ci, max_iter)
print(f"mandelbrot checksum = {checksum}")
