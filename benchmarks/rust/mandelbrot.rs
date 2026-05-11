// rustc -O mandelbrot.rs -o mandelbrot_rs

fn mandel_iter(cr: f64, ci: f64, max_iter: i64) -> i64 {
    let mut zr = 0.0_f64;
    let mut zi = 0.0_f64;
    let mut i: i64 = 0;
    while i < max_iter {
        let zr2 = zr * zr;
        let zi2 = zi * zi;
        if zr2 + zi2 > 4.0 {
            return i;
        }
        let new_zr = zr2 - zi2 + cr;
        let new_zi = 2.0 * zr * zi + ci;
        zr = new_zr;
        zi = new_zi;
        i += 1;
    }
    max_iter
}

fn main() {
    let width: i64 = 200;
    let height: i64 = 200;
    let max_iter: i64 = 1000;
    let mut checksum: i64 = 0;
    let mut y: i64 = 0;
    while y < height {
        let mut x: i64 = 0;
        while x < width {
            let cr = -2.0 + 3.0 * (x as f64) / (width as f64);
            let ci = -1.5 + 3.0 * (y as f64) / (height as f64);
            let it = mandel_iter(cr, ci, max_iter);
            checksum += it;
            x += 1;
        }
        y += 1;
    }
    println!("mandelbrot checksum = {}", checksum);
}
