// CRM-assistant demo for kryos_embed_agent.dll (Go host)
//
// Mirrors the Python smoke_test:
//   (a) Within-budget call (budget_cents=10 >= cost=3): answered=1, source present
//   (b) Over-budget call  (budget_cents=1  < cost=3):  answered=0, spend_cents=0
//   (c) Doctored manifest  (extra cap "net:tcp"):        CapabilityViolation, DLL never loaded
//
// Run from this directory:
//   go run .
//
// Or via check.sh from the repo root.
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"runtime"

	"kryos-embed-go/kryosagent"
)

// repoRoot walks up from this file's location to find the repo root
// (identified by the presence of compiler/).
func repoRoot() string {
	_, file, _, _ := runtime.Caller(0)
	// file is .../hosts/go/main.go -- walk up 4 levels to reach repo root
	d := filepath.Dir(file) // .../hosts/go
	d = filepath.Dir(d)     // .../hosts
	d = filepath.Dir(d)     // .../kryos-embed
	d = filepath.Dir(d)     // .../ecosystem
	d = filepath.Dir(d)     // repo root
	return d
}

func main() {
	root := repoRoot()
	dllPath := filepath.Join(root, "ecosystem", "kryos-embed", "dist", "kryos_embed_agent.dll")
	capsPath := filepath.Join(root, "ecosystem", "kryos-embed", "dist", "agent.caps.json")

	fmt.Println("=== kryos-embed Go CRM-assistant demo ===")
	fmt.Printf("DLL:  %s\n", dllPath)
	fmt.Printf("Caps: %s\n\n", capsPath)

	allowedCaps := []string{"ffi"}

	// ------------------------------------------------------------------ (c)
	// Doctored-manifest refusal: add "net:tcp" to every export, expect gate to fire.
	// ------------------------------------------------------------------ (c)
	fmt.Println("[c] Doctored-manifest refusal (net:tcp added)")
	capsData, err := os.ReadFile(capsPath)
	if err != nil {
		fmt.Fprintf(os.Stderr, "FAIL: reading caps: %v\n", err)
		os.Exit(1)
	}

	var raw map[string]interface{}
	if err := json.Unmarshal(capsData, &raw); err != nil {
		fmt.Fprintf(os.Stderr, "FAIL: parsing caps: %v\n", err)
		os.Exit(1)
	}

	// Deep-doctor: add "net:tcp" to every export's capabilities list.
	if exports, ok := raw["exports"].(map[string]interface{}); ok {
		for _, v := range exports {
			if entry, ok := v.(map[string]interface{}); ok {
				caps, _ := entry["capabilities"].([]interface{})
				entry["capabilities"] = append(caps, "net:tcp")
			}
		}
	}

	doctored, _ := json.Marshal(raw)
	tmpFile, err := os.CreateTemp("", "doctored_caps_*.json")
	if err != nil {
		fmt.Fprintf(os.Stderr, "FAIL: creating temp file: %v\n", err)
		os.Exit(1)
	}
	defer os.Remove(tmpFile.Name())
	if _, err := tmpFile.Write(doctored); err != nil {
		fmt.Fprintf(os.Stderr, "FAIL: writing temp file: %v\n", err)
		os.Exit(1)
	}
	tmpFile.Close()

	err = kryosagent.ParseManifest(tmpFile.Name(), allowedCaps)
	if err == nil {
		fmt.Println("FAIL: expected CapabilityViolation but gate did not fire")
		os.Exit(1)
	}
	if _, ok := err.(*kryosagent.CapabilityViolation); !ok {
		fmt.Printf("FAIL: expected *CapabilityViolation, got %T: %v\n", err, err)
		os.Exit(1)
	}
	fmt.Printf("    gate fired: %v\n", err)
	fmt.Println("    PASS: doctored manifest refused before DLL load")

	// ------------------------------------------------------------------ load real agent
	fmt.Println("\n[load] Parsing real manifest and loading DLL")
	if err := kryosagent.ParseManifest(capsPath, allowedCaps); err != nil {
		fmt.Fprintf(os.Stderr, "FAIL: capability gate rejected real manifest: %v\n", err)
		os.Exit(1)
	}

	agent, err := kryosagent.NewAgent(dllPath)
	if err != nil {
		fmt.Fprintf(os.Stderr, "FAIL: loading agent: %v\n", err)
		os.Exit(1)
	}
	fmt.Println("    PASS: DLL loaded")

	// ------------------------------------------------------------------ (a)
	// Within-budget call: budget_cents=10 >= MOCK_COST_CENTS=3 -> answered=1
	// ------------------------------------------------------------------ (a)
	fmt.Println("\n[a] Within-budget CRM call (budget_cents=10)")
	respA, err := agent.Ask("Which accounts are overdue this quarter?", 10)
	if err != nil {
		fmt.Fprintf(os.Stderr, "FAIL: Ask returned error: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("    response: answered=%d source=%q spend_cents=%d answer=%q\n",
		respA.Answered, respA.Source, respA.SpendCents, respA.Answer)

	if respA.Answered != 1 {
		fmt.Fprintf(os.Stderr, "FAIL: expected answered=1, got %d\n", respA.Answered)
		os.Exit(1)
	}
	if respA.Source == "" {
		fmt.Fprintln(os.Stderr, "FAIL: expected non-empty source")
		os.Exit(1)
	}
	if respA.SpendCents != 3 {
		fmt.Fprintf(os.Stderr, "FAIL: expected spend_cents=3, got %d\n", respA.SpendCents)
		os.Exit(1)
	}
	if respA.Answer == "" {
		fmt.Fprintln(os.Stderr, "FAIL: expected non-empty answer")
		os.Exit(1)
	}
	fmt.Println("    PASS: answered=1, source present, spend_cents=3")

	// ------------------------------------------------------------------ (b)
	// Over-budget call: budget_cents=1 < MOCK_COST_CENTS=3 -> answered=0
	// ------------------------------------------------------------------ (b)
	fmt.Println("\n[b] Over-budget CRM call (budget_cents=1)")
	respB, err := agent.Ask("Summarise all deals from the last 5 years", 1)
	if err != nil {
		fmt.Fprintf(os.Stderr, "FAIL: Ask returned error: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("    response: answered=%d spend_cents=%d reason=%q\n",
		respB.Answered, respB.SpendCents, respB.Reason)

	if respB.Answered != 0 {
		fmt.Fprintf(os.Stderr, "FAIL: expected answered=0, got %d\n", respB.Answered)
		os.Exit(1)
	}
	if respB.SpendCents != 0 {
		fmt.Fprintf(os.Stderr, "FAIL: expected spend_cents=0, got %d\n", respB.SpendCents)
		os.Exit(1)
	}
	if respB.Reason == "" {
		fmt.Fprintln(os.Stderr, "FAIL: expected non-empty reason")
		os.Exit(1)
	}
	fmt.Println("    PASS: answered=0, spend_cents=0, reason present")

	fmt.Println("\n=== ALL ASSERTIONS PASSED ===")
}
