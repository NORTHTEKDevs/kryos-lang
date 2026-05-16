// Benchmark: recursive Fibonacci — mirrors c/fib.c exactly.
// Build: go build -o bin/fib_go go/fib.go
package main

import "fmt"

func fib(n int64) int64 {
	if n < 2 {
		return n
	}
	return fib(n-1) + fib(n-2)
}

func main() {
	n := int64(35)
	r := fib(n)
	fmt.Printf("fib(35) = %d\n", r)
}
