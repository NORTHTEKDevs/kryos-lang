// Benchmark: Mandelbrot set — mirrors c/mandelbrot.c exactly (1000x1000, fp).
// Build: go build -o bin/mandelbrot_go go/mandelbrot.go
package main

import "fmt"

func mandelIter(cr, ci float64, maxIter int64) int64 {
	var zr, zi float64
	for i := int64(0); i < maxIter; i++ {
		zr2 := zr * zr
		zi2 := zi * zi
		if zr2+zi2 > 4.0 {
			return i
		}
		newZr := zr2 - zi2 + cr
		newZi := 2.0*zr*zi + ci
		zr = newZr
		zi = newZi
	}
	return maxIter
}

func main() {
	width, height, maxIter := int64(1000), int64(1000), int64(1000)
	var checksum int64
	for y := int64(0); y < height; y++ {
		for x := int64(0); x < width; x++ {
			cr := -2.0 + 3.0*float64(x)/float64(width)
			ci := -1.5 + 3.0*float64(y)/float64(height)
			checksum += mandelIter(cr, ci, maxIter)
		}
	}
	fmt.Printf("mandelbrot checksum = %d\n", checksum)
}
