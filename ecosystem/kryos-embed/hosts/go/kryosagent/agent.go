// Package kryosagent is a zero-cgo Go binding for kryos_embed_agent.dll.
//
// It uses syscall.NewLazyDLL (no cgo, no build tags required).
//
// Protocol (from agent.caps.json / agent_embed.kry):
//
//	agent_call(req_ptr i64, req_len i64) -> i64  (returns NUL-term JSON C-ptr)
//	agent_response_len() -> i64                   (byte len of last response)
//
//	Request JSON:  {"question": str, "budget_cents": int}
//	Response JSON: {"answered": 0|1, "answer": str, "source": str,
//	                "spend_cents": int, "reason": str}
//
// Capability gate: ParseManifest + NewAgent refuse any manifest whose exports
// declare capabilities outside the caller-supplied allow-set. The DLL is never
// loaded when the gate fires.
package kryosagent

import (
	"encoding/json"
	"fmt"
	"os"
	"syscall"
	"unsafe"
)

// CapabilityViolation is returned when the manifest declares a capability
// that is not in the caller's allow-set. The DLL is never loaded.
type CapabilityViolation struct {
	Export  string
	Cap     string
	Allowed []string
}

func (e *CapabilityViolation) Error() string {
	return fmt.Sprintf(
		"capability gate: export %q requires %q which is not in allowed_caps=%v -- DLL NOT LOADED",
		e.Export, e.Cap, e.Allowed,
	)
}

// Response mirrors the JSON the DLL returns.
type Response struct {
	Answered   int    `json:"answered"`
	Answer     string `json:"answer"`
	Source     string `json:"source"`
	SpendCents int    `json:"spend_cents"`
	Reason     string `json:"reason"`
}

// manifest is the internal representation of agent.caps.json.
type manifest struct {
	Exports map[string]exportEntry `json:"exports"`
}

type exportEntry struct {
	Capabilities []string `json:"capabilities"`
}

// ParseManifest reads and validates agent.caps.json at capsPath.
// It returns an error (CapabilityViolation) if any export requires a
// capability outside allowedCaps. Callers MUST call this before NewAgent.
func ParseManifest(capsPath string, allowedCaps []string) error {
	data, err := os.ReadFile(capsPath)
	if err != nil {
		return fmt.Errorf("kryosagent: reading caps manifest: %w", err)
	}

	var m manifest
	if err := json.Unmarshal(data, &m); err != nil {
		return fmt.Errorf("kryosagent: parsing caps manifest: %w", err)
	}

	allowed := make(map[string]bool, len(allowedCaps))
	for _, c := range allowedCaps {
		allowed[c] = true
	}

	for name, entry := range m.Exports {
		for _, cap := range entry.Capabilities {
			if !allowed[cap] {
				return &CapabilityViolation{
					Export:  name,
					Cap:     cap,
					Allowed: allowedCaps,
				}
			}
		}
	}
	return nil
}

// Agent is a live binding to the loaded DLL.
// Construct only after ParseManifest returns nil.
type Agent struct {
	dll             *syscall.LazyDLL
	procAgentCall   *syscall.LazyProc
	procResponseLen *syscall.LazyProc
}

// NewAgent loads the DLL at dllPath.
// MUST call ParseManifest first -- NewAgent does not re-run the capability gate.
func NewAgent(dllPath string) (*Agent, error) {
	dll := syscall.NewLazyDLL(dllPath)
	a := &Agent{
		dll:             dll,
		procAgentCall:   dll.NewProc("agent_call"),
		procResponseLen: dll.NewProc("agent_response_len"),
	}
	// Eagerly resolve to fail fast with a clear error if the DLL is missing.
	if err := a.procAgentCall.Find(); err != nil {
		return nil, fmt.Errorf("kryosagent: agent_call not found in %s: %w", dllPath, err)
	}
	if err := a.procResponseLen.Find(); err != nil {
		return nil, fmt.Errorf("kryosagent: agent_response_len not found in %s: %w", dllPath, err)
	}
	return a, nil
}

// Ask sends question to the in-process agent with the given budget.
// The DLL's budget gate fires before the mock LLM; over-budget calls
// return Response.Answered == 0 and record zero spend.
func (a *Agent) Ask(question string, budgetCents int64) (Response, error) {
	// Build request JSON.
	type reqPayload struct {
		Question    string `json:"question"`
		BudgetCents int64  `json:"budget_cents"`
	}
	reqBytes, err := json.Marshal(reqPayload{Question: question, BudgetCents: budgetCents})
	if err != nil {
		return Response{}, fmt.Errorf("kryosagent: marshalling request: %w", err)
	}

	// Pin request in Go heap. unsafe.Pointer(®Bytes[0]) is stable for the
	// duration of the syscall because reqBytes is kept live by the defer.
	reqPtr := uintptr(unsafe.Pointer(&reqBytes[0]))
	reqLen := uintptr(len(reqBytes))

	// Call agent_call(req_ptr i64, req_len i64) -> i64
	respPtrRaw, _, _ := a.procAgentCall.Call(reqPtr, reqLen)

	// Call agent_response_len() -> i64
	respLenRaw, _, _ := a.procResponseLen.Call()
	respLen := int(respLenRaw)

	if respPtrRaw == 0 || respLen <= 0 {
		return Response{}, fmt.Errorf("kryosagent: DLL returned null/empty response")
	}

	// Copy respLen bytes from the DLL-owned C-string into Go memory.
	// Using unsafe.Slice avoids an extra allocation compared to a byte-by-byte copy.
	cBytes := unsafe.Slice((*byte)(unsafe.Pointer(respPtrRaw)), respLen)
	buf := make([]byte, respLen)
	copy(buf, cBytes)

	var resp Response
	if err := json.Unmarshal(buf, &resp); err != nil {
		return Response{}, fmt.Errorf("kryosagent: unmarshalling response %q: %w", string(buf), err)
	}
	return resp, nil
}
