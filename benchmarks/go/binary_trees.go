// Benchmark: binary trees — pure recursion; mirrors c/binary_trees.c exactly.
// Build: go build -o bin/binary_trees_go go/binary_trees.go
package main

import "fmt"

func treeSum(depth, value int64) int64 {
	if depth == 0 {
		return value
	}
	return treeSum(depth-1, value*2) + treeSum(depth-1, value*2+1)
}

func main() {
	s := treeSum(18, 1)
	fmt.Printf("binary_trees(depth=21) = %d\n", s)
}
