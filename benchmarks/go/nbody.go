// Benchmark: N-body simulation — mirrors c/nbody.c exactly.
// Build: go build -o bin/nbody_go go/nbody.go
package main

import (
	"fmt"
	"math"
)

func main() {
	const n = 5
	const steps = int64(50000)
	const dt = 0.01

	x := [n]float64{0.0, 1.0, 2.0, 3.0, 4.0}
	y := [n]float64{0.0, 0.5, 1.0, 1.5, 2.0}
	z := [n]float64{}
	vx := [n]float64{}
	vy := [n]float64{}
	vz := [n]float64{}
	mass := [n]float64{1.0, 0.5, 0.5, 0.3, 0.2}

	for s := int64(0); s < steps; s++ {
		for i := 0; i < n; i++ {
			var fx, fy, fz float64
			for j := 0; j < n; j++ {
				if i != j {
					dx := x[j] - x[i]
					dy := y[j] - y[i]
					dz := z[j] - z[i]
					r2 := dx*dx + dy*dy + dz*dz + 0.001
					r := math.Sqrt(r2)
					invR3 := mass[j] / (r2 * r)
					fx += dx * invR3
					fy += dy * invR3
					fz += dz * invR3
				}
			}
			vx[i] += fx * dt
			vy[i] += fy * dt
			vz[i] += fz * dt
		}
		for k := 0; k < n; k++ {
			x[k] += vx[k] * dt
			y[k] += vy[k] * dt
			z[k] += vz[k] * dt
		}
	}

	var checksum float64
	for k := 0; k < n; k++ {
		checksum += x[k] + y[k] + z[k]
	}
	fmt.Printf("nbody checksum (rounded) = %d\n", int64(checksum))
}
