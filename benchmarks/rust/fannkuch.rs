// rustc -O fannkuch.rs -o fannkuch_rs

fn count_flips(perm: &mut [i64], _n: usize) -> i64 {
    let mut flips = 0;
    let mut p0 = perm[0];
    while p0 != 0 {
        let mut a = 0_usize;
        let mut b = p0 as usize;
        while a < b {
            perm.swap(a, b);
            a += 1;
            b -= 1;
        }
        flips += 1;
        p0 = perm[0];
    }
    flips
}

fn rotate_left(perm: &mut [i64], n: usize) {
    let first = perm[0];
    for i in 0..n - 1 {
        perm[i] = perm[i + 1];
    }
    perm[n - 1] = first;
}

fn main() {
    let n: usize = 10;
    let mut perm: Vec<i64> = (0..n as i64).collect();
    let mut max_flips: i64 = 0;
    let total = 362880;
    for _ in 0..total {
        let mut work = perm.clone();
        let f = count_flips(&mut work, n);
        if f > max_flips {
            max_flips = f;
        }
        rotate_left(&mut perm, n);
    }
    println!("fannkuch(10) = {}", max_flips);
}
