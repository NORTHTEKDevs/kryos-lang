// go build -o fannkuch_go fannkuch.go
// Canonical fannkuch-redux (Benchmarks Game shape), single-threaded.
package main

import "fmt"

func main() {
	const n = 10
	perm1 := make([]int64, n)
	count := make([]int64, n)
	perm := make([]int64, n)
	for i := 0; i < n; i++ {
		perm1[i] = int64(i)
	}
	var maxFlips, checksum, permCount int64
	r := n
	for {
		for r != 1 {
			count[r-1] = int64(r)
			r--
		}
		copy(perm, perm1)
		var flips int64
		k := perm[0]
		for k != 0 {
			a, b := 0, int(k)
			for a < b {
				perm[a], perm[b] = perm[b], perm[a]
				a++
				b--
			}
			flips++
			k = perm[0]
		}
		if flips > maxFlips {
			maxFlips = flips
		}
		if permCount%2 == 0 {
			checksum += flips
		} else {
			checksum -= flips
		}
		for {
			if r == n {
				fmt.Println(checksum)
				fmt.Printf("Pfannkuchen(%d) = %d\n", n, maxFlips)
				return
			}
			perm0 := perm1[0]
			for i := 0; i < r; i++ {
				perm1[i] = perm1[i+1]
			}
			perm1[r] = perm0
			count[r]--
			if count[r] > 0 {
				break
			}
			r++
		}
		permCount++
	}
}
