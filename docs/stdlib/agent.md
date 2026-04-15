# std::agent

A first-class framework for building autonomous AI agents. Provides tool registration, memory management, alignment controls, audit trails, and agent orchestration via swarms.

```kryos
use std::agent
```

---

## Constants

### Alignment

Controls the behavioral constraints applied to an agent.

| Constant                  | Value | Description                                           |
|---------------------------|-------|-------------------------------------------------------|
| `ALIGNMENT_STRICT`        | `0`   | Maximum constraints -- refuses ambiguous or risky actions |
| `ALIGNMENT_STANDARD`      | `1`   | Balanced defaults (recommended for most agents)       |
| `ALIGNMENT_MINIMAL`       | `2`   | Minimal constraints -- agent exercises more autonomy  |
| `ALIGNMENT_UNRESTRICTED`  | `3`   | No alignment enforcement -- use only in sandboxed environments |

### State

Lifecycle state of an agent.

| Constant            | Value | Description                     |
|---------------------|-------|---------------------------------|
| `STATE_CREATED`     | `0`   | Agent initialized, not yet run  |
| `STATE_RUNNING`     | `1`   | Actively executing              |
| `STATE_PAUSED`      | `2`   | Execution suspended             |
| `STATE_COMPLETED`   | `3`   | Finished successfully           |
| `STATE_FAILED`      | `4`   | Terminated due to error         |
| `STATE_TERMINATED`  | `5`   | Manually stopped                |

---

## Types

### MemoryEntry

```kryos
struct MemoryEntry {
    key:         str,
    value:       any,
    memory_type: str,
    timestamp:   i64
}
```

`memory_type` is a free-form label (e.g. `"working"`, `"semantic"`, `"episodic"`).

---

### AgentMemory

Three-tier memory store for an agent.

```kryos
struct AgentMemory {
    working:   [MemoryEntry],
    semantic:  [MemoryEntry],
    episodic:  [MemoryEntry]
}
```

---

### AuditEntry

One record in an agent's immutable audit trail.

```kryos
struct AuditEntry {
    id:          str,
    entry_type:  str,
    description: str,
    success:     bool,
    timestamp:   i64,
    cost_usd:    f64,
    latency_ms:  f64
}
```

---

### AgentTool

A callable tool registered on an agent.

```kryos
struct AgentTool {
    name:        str,
    description: str,
    handler:     fn(str) -> str
}
```

---

### Agent

```kryos
struct Agent {
    name:         str,
    goal:         str,
    alignment:    i64,
    state:        i64,
    memory:       AgentMemory,
    tools:        [AgentTool],
    audit_trail:  [AuditEntry],
    action_count: i64,
    total_cost:   f64,
    capabilities: [str]
}
```

---

## Memory Functions

### agent_memory_new

`agent_memory_new() -> AgentMemory`

Create an empty three-tier memory store.

---

### remember (method on AgentMemory)

`remember(key: str, value: any, memory_type: str)`

Store a value under `key` in the specified memory tier.

---

### recall (method on AgentMemory)

`recall(key: str) -> any`

Return the value stored under `key`, searching all three tiers. Returns `null` if not found.

---

### clear_working (method on AgentMemory)

`clear_working()`

Clear all entries from the working memory tier.

---

## Agent Construction

### agent_new

`agent_new(name: str, goal: str) -> Agent`

Create an agent with `ALIGNMENT_STANDARD`.

---

### agent_with_alignment

`agent_with_alignment(name: str, goal: str, alignment: i64) -> Agent`

Create an agent with an explicit alignment level.

**Example:**
```kryos
use std::agent

let a = agent_new("researcher", "summarize recent papers")
let strict = agent_with_alignment("auditor", "review financial records", ALIGNMENT_STRICT)
```

---

## Agent Methods

### add_tool

`add_tool(name: str, handler: fn(str) -> str, description: str) -> Agent`

Register a tool on the agent. Returns `self` for chaining. The `handler` receives a JSON string of inputs and returns a JSON string result.

**Example:**
```kryos
use std::agent

let a = agent_new("assistant", "answer questions")
    .add_tool("calculator", fn(input: str) -> str {
        // parse input, compute, return JSON result
        return "{\"result\": 42}"
    }, "evaluate arithmetic expressions")
```

---

### use_tool

`use_tool(tool_name: str, input: str) -> str`

Invoke the named tool with `input` (a JSON string). Records an `AuditEntry`. Throws if the tool is not registered or the agent is not in `STATE_RUNNING`.

---

### spawn_child

`spawn_child(name: str, goal: str) -> Agent`

Create a new sub-agent that inherits the parent's alignment and tool registry. Records the spawn in the parent's audit trail.

---

### pause

`pause()`

Transition the agent to `STATE_PAUSED`.

---

### resume

`resume()`

Transition the agent from `STATE_PAUSED` back to `STATE_RUNNING`.

---

### terminate

`terminate()`

Transition the agent to `STATE_TERMINATED` and clear its working memory.

---

### get_audit_trail

`get_audit_trail() -> [AuditEntry]`

Return a copy of the agent's immutable audit trail.

---

### status

`status() -> str`

Return a human-readable status summary including name, goal, state, action count, and total cost.

---

## AgentSwarm

A collection of named agents that can be managed together.

### agent_swarm

`agent_swarm(name: str) -> AgentSwarm`

Create a named swarm.

---

### add (method on AgentSwarm)

`add(agent: Agent)`

Register an agent with the swarm.

---

### terminate_all (method on AgentSwarm)

`terminate_all()`

Call `terminate()` on every agent in the swarm.

---

## Complete Example

```kryos
use std::agent

// Build an agent with tools
let researcher = agent_new("researcher", "find and summarize information")
    .add_tool(
        "web_search",
        fn(query: str) -> str {
            // In practice: call a search API
            return "{\"results\": [\"result 1\", \"result 2\"]}"
        },
        "search the web for information"
    )
    .add_tool(
        "summarize",
        fn(text: str) -> str {
            return "{\"summary\": \"Key points extracted.\"}"
        },
        "summarize a block of text"
    )

// Use tools
let search_result = researcher.use_tool("web_search", "{\"query\": \"Kryos language\"}")
println(search_result)

// Spawn a child agent for a subtask
let writer = researcher.spawn_child("writer", "format findings as a report")

// Audit
let trail = researcher.get_audit_trail()
println(len(trail))   // 2 (web_search call + spawn_child)

println(researcher.status())

// Swarm management
let swarm = agent_swarm("pipeline")
swarm.add(researcher)
swarm.add(writer)

// ... run pipeline ...

swarm.terminate_all()
```
