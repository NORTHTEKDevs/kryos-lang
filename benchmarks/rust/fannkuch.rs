// rustc -O fannkuch.rs -o fannkuch_rust
// Canonical fannkuch-redux (Benchmarks Game shape), single-threaded.
fn main() {
    let n: usize = 10;
    let mut perm1: Vec<i64> = (0..n as i64).collect();
    let mut count = vec![0i64; n];
    let mut perm = vec![0i64; n];
    let mut max_flips: i64 = 0;
    let mut checksum: i64 = 0;
    let mut perm_count: i64 = 0;
    let mut r = n;
    loop {
        while r != 1 {
            count[r - 1] = r as i64;
            r -= 1;
        }
        perm.copy_from_slice(&perm1);
        let mut flips: i64 = 0;
        let mut k = perm[0];
        while k != 0 {
            perm[..(k as usize + 1)].reverse();
            flips += 1;
            k = perm[0];
        }
        if flips > max_flips {
            max_flips = flips;
        }
        checksum += if perm_count % 2 == 0 { flips } else { -flips };
        loop {
            if r == n {
                println!("{}", checksum);
                println!("Pfannkuchen({}) = {}", n, max_flips);
                return;
            }
            let perm0 = perm1[0];
            for i in 0..r {
                perm1[i] = perm1[i + 1];
            }
            perm1[r] = perm0;
            count[r] -= 1;
            if count[r] > 0 {
                break;
            }
            r += 1;
        }
        perm_count += 1;
    }
}
