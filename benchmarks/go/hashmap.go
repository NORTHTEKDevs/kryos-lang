package main
import "fmt"
func main() {
    n := int64(1000000)
    m := make(map[int64]int64, n)
    for i := int64(0); i < n; i++ { m[i] = i * 2 }
    var sum int64
    for j := int64(0); j < n; j++ { sum += m[j] }
    fmt.Println(sum)
}
