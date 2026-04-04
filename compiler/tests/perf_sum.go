package main

import "fmt"

func main() {
	var sum int64 = 0
	for i := int64(0); i < 100000000; i++ {
		sum += i
	}
	_ = sum
	fmt.Println("sum done")
}
