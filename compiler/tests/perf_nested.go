package main

import "fmt"

func main() {
	var total int64 = 0
	for i := int64(0); i < 1000; i++ {
		for j := int64(0); j < 1000; j++ {
			total += i * j
		}
	}
	_ = total
	fmt.Println("nested done")
}
