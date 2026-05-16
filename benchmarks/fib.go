package main
import "fmt"
func fib(n int64) int64 { if n <= 1 { return n }; return fib(n-1) + fib(n-2) }
func main() { fmt.Printf("fib(40) = %d\n", fib(40)) }
