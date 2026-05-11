// rustc -O matmul.rs -o matmul_rs

fn main() {
    let n: usize = 256;
    let mut a = vec![0_i64; n * n];
    let mut b = vec![0_i64; n * n];
    let mut c = vec![0_i64; n * n];
    for i in 0..n * n {
        a[i] = ((i as i64) * 3) % 7;
        b[i] = ((i as i64) * 5) % 11;
    }
    for row in 0..n {
        for col in 0..n {
            let mut sum: i64 = 0;
            for k in 0..n {
                sum += a[row * n + k] * b[k * n + col];
            }
            c[row * n + col] = sum;
        }
    }
    let checksum: i64 = c.iter().sum();
    println!("matmul(256) checksum = {}", checksum);
}
