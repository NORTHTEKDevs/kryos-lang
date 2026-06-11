// Canonical allocation-stress binary trees (same algorithm as kryos port).
package main

import "fmt"

type Tree struct{ left, right *Tree }

func make_(depth int64) *Tree {
	if depth == 0 {
		return &Tree{}
	}
	return &Tree{left: make_(depth - 1), right: make_(depth - 1)}
}

func check(t *Tree) int64 {
	if t.left == nil {
		return 1
	}
	return 1 + check(t.left) + check(t.right)
}

func main() {
	var maxDepth int64 = 16
	longLived := make_(maxDepth)
	var total int64 = 0
	for depth := int64(4); depth <= maxDepth; depth += 2 {
		iterations := int64(1) << (maxDepth - depth + 4)
		var sum int64 = 0
		for i := int64(0); i < iterations; i++ {
			sum += check(make_(depth))
		}
		total += sum
	}
	total += check(longLived)
	fmt.Printf("binary_trees(canonical, depth=16) checksum = %d\n", total)
}
