package main

import (
	"fmt"
	"strings"
)

func main() {
	n := 100000
	token := "ab"
	var b strings.Builder
	for i := 0; i < n; i++ {
		b.WriteString(token)
	}
	s := b.String()
	slen := len(s)
	var checksum int64
	for _, c := range []byte(s) {
		checksum += int64(c)
	}
	fmt.Println(slen)
	fmt.Println(checksum)
}
