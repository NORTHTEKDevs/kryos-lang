fn mandel(cr: i64, ci: i64, max_iter: i64, scale: i64) -> i64 {
    let mut zr: i64 = 0;
    let mut zi: i64 = 0;
    let mut i: i64 = 0;
    let four = 4 * scale * scale;
    while i < max_iter {
        let zr2 = (zr * zr) / scale;
        let zi2 = (zi * zi) / scale;
        if zr2 + zi2 > four { return i; }
        let new_zr = zr2 - zi2 + cr;
        let new_zi = (2 * zr * zi) / scale + ci;
        zr = new_zr; zi = new_zi; i += 1;
    }
    max_iter
}
fn main() {
    let scale: i64 = 1000;
    let (w, h, max_iter): (i64, i64, i64) = (800, 800, 1000);
    let (xmin, xmax, ymin, ymax) = (-2*scale, 1*scale, -1*scale, 1*scale);
    let mut total: i64 = 0;
    for y in 0..h {
        let ci = ymin + ((ymax-ymin) * y) / h;
        for x in 0..w {
            let cr = xmin + ((xmax-xmin) * x) / w;
            total += mandel(cr, ci, max_iter, scale);
        }
    }
    println!("total iterations = {}", total);
}
