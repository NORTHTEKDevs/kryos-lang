def mandel(cr, ci, max_iter, scale):
    zr = 0; zi = 0; i = 0
    four = 4 * scale * scale
    while i < max_iter:
        zr2 = (zr * zr) // scale
        zi2 = (zi * zi) // scale
        if zr2 + zi2 > four: return i
        new_zr = zr2 - zi2 + cr
        new_zi = (2 * zr * zi) // scale + ci
        zr = new_zr; zi = new_zi; i += 1
    return max_iter

scale = 1000; w = 800; h = 800; max_iter = 1000
xmin, xmax, ymin, ymax = -2*scale, 1*scale, -1*scale, 1*scale
total = 0
for y in range(h):
    ci = ymin + ((ymax-ymin) * y) // h
    for x in range(w):
        cr = xmin + ((xmax-xmin) * x) // w
        total += mandel(cr, ci, max_iter, scale)
print(f"total iterations = {total}")
