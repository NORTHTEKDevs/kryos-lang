// Benchmark: Fannkuch-redux — mirrors c/fannkuch.c exactly.
// Build: go build -o bin/fannkuch_go go/fannkuch.go
package main

import "fmt"

func countFlips(perm []int64) int64 {
	var flips int64
	p0 := perm[0]
	for p0 != 0 {
		a, b := 0, int(p0)
		for a < b {
			perm[a], perm[b] = perm[b], perm[a]
			a++
			b--
		}
		flips++
		p0 = perm[0]
	}
	return flips
}

func rotateLeft(perm []int64) {
	n := len(perm)
	first := perm[0]
	for i := 0; i < n-1; i++ {
		perm[i] = perm[i+1]
	}
	perm[n-1] = first
}

func main() {
	const n = 10
	perm := make([]int64, n)
	for i := range perm {
		perm[i] = int64(i)
	}
	var maxFlips int64
	const total = 362880 // 9!
	work := make([]int64, n)
	for iter := 0; iter < total; iter++ {
		copy(work, perm)
		f := countFlips(work)
		if f > maxFlips {
			maxFlips = f
		}
		rotateLeft(perm)
	}
	fmt.Printf("fannkuch(10) = %d\n", maxFlips)
}
