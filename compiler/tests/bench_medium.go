package main

type Point struct {
	X float64
	Y float64
}

func distance(a, b Point) float64 {
	dx := a.X - b.X
	dy := a.Y - b.Y
	return dx*dx + dy*dy
}

func compute(n int) float64 {
	var total float64
	p1 := Point{1.0, 2.0}
	p2 := Point{4.0, 6.0}
	for i := 0; i < n; i++ {
		total += distance(p1, p2)
	}
	return total
}

func main() {
	_ = compute(1000)
}
