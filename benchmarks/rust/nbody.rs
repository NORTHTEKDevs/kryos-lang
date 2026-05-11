// rustc -O nbody.rs -o nbody_rs

fn main() {
    let n = 5_usize;
    let steps: i64 = 50000;
    let dt = 0.01_f64;

    let mut x: [f64; 5] = [0.0, 1.0, 2.0, 3.0, 4.0];
    let mut y: [f64; 5] = [0.0, 0.5, 1.0, 1.5, 2.0];
    let mut z: [f64; 5] = [0.0; 5];
    let mut vx: [f64; 5] = [0.0; 5];
    let mut vy: [f64; 5] = [0.0; 5];
    let mut vz: [f64; 5] = [0.0; 5];
    let mass: [f64; 5] = [1.0, 0.5, 0.5, 0.3, 0.2];

    for _ in 0..steps {
        for i in 0..n {
            let mut fx = 0.0;
            let mut fy = 0.0;
            let mut fz = 0.0;
            for j in 0..n {
                if i != j {
                    let dx = x[j] - x[i];
                    let dy = y[j] - y[i];
                    let dz = z[j] - z[i];
                    let r2 = dx * dx + dy * dy + dz * dz + 0.001;
                    let r = r2.sqrt();
                    let inv_r3 = mass[j] / (r2 * r);
                    fx += dx * inv_r3;
                    fy += dy * inv_r3;
                    fz += dz * inv_r3;
                }
            }
            vx[i] += fx * dt;
            vy[i] += fy * dt;
            vz[i] += fz * dt;
        }
        for k in 0..n {
            x[k] += vx[k] * dt;
            y[k] += vy[k] * dt;
            z[k] += vz[k] * dt;
        }
    }

    let mut checksum: f64 = 0.0;
    for k in 0..n {
        checksum += x[k] + y[k] + z[k];
    }
    println!("nbody checksum (rounded) = {}", checksum as i64);
}
