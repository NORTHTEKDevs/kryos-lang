struct Point {
    x: f64,
    y: f64,
}

fn distance(a: &Point, b: &Point) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy
}

fn compute(n: i32) -> f64 {
    let mut total: f64 = 0.0;
    let p1 = Point { x: 1.0, y: 2.0 };
    let p2 = Point { x: 4.0, y: 6.0 };
    for _ in 0..n {
        total += distance(&p1, &p2);
    }
    total
}

fn main() {
    let _result = compute(1000);
}
