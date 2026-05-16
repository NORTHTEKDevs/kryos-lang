// Benchmark: integer matrix multiplication 256×256 — mirrors c/matmul.c exactly.
// Build: go build -o bin/matmul_go go/matmul.go
package main

import "fmt"

func main() {
	const n = 256
	a := make([]int64, n*n)
	b := make([]int64, n*n)
	c := make([]int64, n*n)
	for i := 0; i < n*n; i++ {
		a[i] = (int64(i) * 3) % 7
		b[i] = (int64(i) * 5) % 11
	}
	for row := 0; row < n; row++ {
		for col := 0; col < n; col++ {
			var sum int64
			for k := 0; k < n; k++ {
				sum += a[row*n+k] * b[k*n+col]
			}
			c[row*n+col] = sum
		}
	}
	var checksum int64
	for i := 0; i < n*n; i++ {
		checksum += c[i]
	}
	fmt.Printf("matmul(256) checksum = %d\n", checksum)
}
