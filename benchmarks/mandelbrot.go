package main
import "fmt"
func mandel(cr, ci, maxIter, scale int64) int64 {
	var zr, zi, i int64 = 0, 0, 0
	four := 4 * scale * scale
	for i < maxIter {
		zr2 := (zr * zr) / scale
		zi2 := (zi * zi) / scale
		if zr2+zi2 > four { return i }
		newZr := zr2 - zi2 + cr
		newZi := (2*zr*zi)/scale + ci
		zr = newZr; zi = newZi; i++
	}
	return maxIter
}
func main() {
	var scale, w, h, maxIter int64 = 1000, 800, 800, 1000
	xmin, xmax, ymin, ymax := -2*scale, 1*scale, -1*scale, 1*scale
	var total int64 = 0
	for y := int64(0); y < h; y++ {
		ci := ymin + ((ymax-ymin)*y)/h
		for x := int64(0); x < w; x++ {
			cr := xmin + ((xmax-xmin)*x)/w
			total += mandel(cr, ci, maxIter, scale)
		}
	}
	fmt.Printf("total iterations = %d\n", total)
}
