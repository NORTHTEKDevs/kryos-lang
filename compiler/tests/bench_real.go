package main

import "fmt"

func add(a, b int32) int32 {
	return a + b
}

func multiply(a, b int32) int32 {
	return a * b
}

func main() {
	x := add(10, 32)
	_ = multiply(x, 2)
	fmt.Println("Kryos is alive")
}
