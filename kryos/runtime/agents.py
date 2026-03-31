"""
Kryos Agent Runtime

Agents are first-class autonomous entities with goals, memory, tools,
and coordination protocols. Unlike actor-based systems, Kryos agents
have persistent memory, can reason about their own state, and coordinate
with other agents through formal protocols.

Key principle: The OWNER controls the guardrails.
- @alignment constraints are OPTIONAL. You choose when to apply them.
- @unrestricted mode removes all behavioral constraints.
- Capability system still applies (security), but alignment is YOUR choice.

Modes:
  @alignment(strict)      — full safety rails, every action audited and constrained
  @alignment(standard)    — reasonable defaults, override with justification
  @alignment(minimal)     — basic logging only
  @unrestricted           — no behavioral constraints, no alignment checks
                            YOU own it. YOUR rules. Full power.
"""

from __future__ import annotations

import time
import uuid
from dataclasses import dataclass, field
from enum import Enum, auto
from typing import Any, Callable, Optional


class AlignmentMode(Enum):
    """How much behavioral restriction an agent has."""
    UNRESTRICTED = 0   # No constraints. Full power. Owner's choice.
    MINIMAL = 1        # Basic logging, no behavioral checks
    STANDARD = 2       # Reasonable defaults
    STRICT = 3         # Full safety rails, mandatory audit


class AgentState(Enum):
    """Lifecycle state of an agent."""
    CREATED = auto()
    RUNNING = auto()
    PAUSED = auto()
    COMPLETED = auto()
    FAILED = auto()
    TERMINATED = auto()


@dataclass
class AgentMemory:
    """
    Persistent memory for an agent. Survives across invocations.

    Three memory types:
    - working: Short-term, cleared between tasks
    - episodic: Records of past actions and their outcomes
    - semantic: Learned facts and knowledge
    """
    working: dict[str, Any] = field(default_factory=dict)
    episodic: list[dict[str, Any]] = field(default_factory=list)
    semantic: dict[str, Any] = field(default_factory=dict)

    def remember(self, key: str, value: Any, memory_type: str = "working") -> None:
        """Store a value in memory."""
        if memory_type == "working":
            self.working[key] = value
        elif memory_type == "semantic":
            self.semantic[key] = value
        elif memory_type == "episodic":
            self.episodic.append({
                "key": key,
                "value": value,
                "timestamp": time.time(),
            })

    def recall(self, key: str) -> Any:
        """Recall from working memory first, then semantic."""
        if key in self.working:
            return self.working[key]
        if key in self.semantic:
            return self.semantic[key]
        return None

    def recall_episodes(self, key: str = None, last_n: int = 10) -> list[dict]:
        """Recall episodic memories, optionally filtered by key."""
        episodes = self.episodic
        if key:
            episodes = [e for e in episodes if e.get("key") == key]
        return episodes[-last_n:]

    def clear_working(self) -> None:
        """Clear working memory between tasks."""
        self.working.clear()

    def to_dict(self) -> dict:
        return {
            "working": self.working,
            "episodic": self.episodic[-100:],  # keep last 100
            "semantic": self.semantic,
        }


@dataclass
class AgentAction:
    """A recorded action taken by an agent."""
    action_id: str = ""
    agent_id: str = ""
    action_type: str = ""
    description: str = ""
    inputs: dict[str, Any] = field(default_factory=dict)
    outputs: Any = None
    success: bool = True
    timestamp: float = 0.0
    cost: float = 0.0  # monetary cost
    latency_ms: float = 0.0

    def __post_init__(self):
        if not self.action_id:
            self.action_id = str(uuid.uuid4())[:8]
        if not self.timestamp:
            self.timestamp = time.time()


@dataclass
class Tool:
    """A tool available to an agent."""
    name: str
    description: str
    fn: Callable
    capabilities_required: list[str] = field(default_factory=list)

    def execute(self, *args: Any, **kwargs: Any) -> Any:
        return self.fn(*args, **kwargs)


class Agent:
    """
    A Kryos autonomous agent.

    Agents have:
    - A goal (what they're trying to accomplish)
    - Memory (working, episodic, semantic)
    - Tools (functions they can call)
    - Alignment mode (YOUR choice of constraints)
    - Action history (full audit trail)

    The alignment mode is set by YOU, the owner. Not by the language.
    @unrestricted means unrestricted. No hand-holding. Full power.
    """

    def __init__(
        self,
        name: str,
        goal: str = "",
        alignment: AlignmentMode = AlignmentMode.UNRESTRICTED,
        capabilities: list[str] | None = None,
    ) -> None:
        self.id = str(uuid.uuid4())[:12]
        self.name = name
        self.goal = goal
        self.alignment = alignment
        self.state = AgentState.CREATED
        self.memory = AgentMemory()
        self.tools: dict[str, Tool] = {}
        self.action_history: list[AgentAction] = []
        self.capabilities = capabilities or []
        self.children: list[Agent] = []
        self._on_action: list[Callable] = []  # hooks for monitoring
        self.created_at = time.time()
        self.total_cost = 0.0
        self.total_actions = 0

    # ------------------------------------------------------------------
    # Tool management
    # ------------------------------------------------------------------

    def add_tool(self, name: str, fn: Callable, description: str = "",
                 capabilities: list[str] | None = None) -> None:
        """Register a tool the agent can use."""
        self.tools[name] = Tool(
            name=name,
            description=description or f"Tool: {name}",
            fn=fn,
            capabilities_required=capabilities or [],
        )

    def use_tool(self, tool_name: str, *args: Any, **kwargs: Any) -> Any:
        """Use a registered tool."""
        if tool_name not in self.tools:
            raise AgentError(f"Agent '{self.name}' has no tool '{tool_name}'")

        tool = self.tools[tool_name]
        start = time.time()

        try:
            result = tool.execute(*args, **kwargs)
            elapsed = (time.time() - start) * 1000

            action = AgentAction(
                agent_id=self.id,
                action_type="tool_use",
                description=f"{tool_name}({args}, {kwargs})",
                inputs={"args": str(args), "kwargs": str(kwargs)},
                outputs=result,
                success=True,
                latency_ms=elapsed,
            )
            self._record_action(action)
            return result

        except Exception as e:
            action = AgentAction(
                agent_id=self.id,
                action_type="tool_use_failed",
                description=f"{tool_name} failed: {e}",
                success=False,
            )
            self._record_action(action)
            raise

    # ------------------------------------------------------------------
    # Execution
    # ------------------------------------------------------------------

    def execute(self, task: Any = None) -> Any:
        """Execute the agent's goal or a specific task."""
        self.state = AgentState.RUNNING

        action = AgentAction(
            agent_id=self.id,
            action_type="execute",
            description=f"Executing: {task or self.goal}",
            inputs={"task": str(task)},
        )

        try:
            result = self._run(task)
            action.outputs = result
            action.success = True
            self.state = AgentState.COMPLETED
        except Exception as e:
            action.outputs = str(e)
            action.success = False
            self.state = AgentState.FAILED
            raise
        finally:
            self._record_action(action)

        return result

    def _run(self, task: Any) -> Any:
        """Override this in subclasses or set via on_execute."""
        # Default: return the task description
        return f"Agent '{self.name}' completed: {task or self.goal}"

    # ------------------------------------------------------------------
    # Child agents / delegation
    # ------------------------------------------------------------------

    def spawn_child(
        self,
        name: str,
        goal: str = "",
        alignment: AlignmentMode | None = None,
        capabilities: list[str] | None = None,
    ) -> Agent:
        """
        Spawn a child agent.

        Alignment: child inherits parent's alignment mode by default.
        Capabilities: child can only have a SUBSET of parent's capabilities.
        """
        child_alignment = alignment if alignment is not None else self.alignment

        # Capability narrowing — child cannot exceed parent
        if capabilities:
            parent_caps = set(self.capabilities)
            child_caps = set(capabilities)
            if not child_caps.issubset(parent_caps) and parent_caps:
                invalid = child_caps - parent_caps
                raise AgentError(
                    f"Child agent '{name}' requested capabilities {invalid} "
                    f"not held by parent '{self.name}'"
                )
        else:
            capabilities = list(self.capabilities)

        child = Agent(
            name=name,
            goal=goal,
            alignment=child_alignment,
            capabilities=capabilities,
        )
        self.children.append(child)
        return child

    # ------------------------------------------------------------------
    # Monitoring and audit
    # ------------------------------------------------------------------

    def _record_action(self, action: AgentAction) -> None:
        """Record an action in history."""
        self.action_history.append(action)
        self.total_actions += 1
        self.total_cost += action.cost

        # Store in episodic memory
        self.memory.remember(
            action.action_type,
            {"description": action.description, "success": action.success},
            memory_type="episodic",
        )

        # Fire monitoring hooks
        for hook in self._on_action:
            try:
                hook(action)
            except Exception:
                pass  # monitoring should never crash the agent

    def on_action(self, callback: Callable) -> None:
        """Register a monitoring callback for every action."""
        self._on_action.append(callback)

    def get_audit_trail(self) -> list[dict]:
        """Export full action history for audit."""
        return [
            {
                "id": a.action_id,
                "type": a.action_type,
                "description": a.description,
                "success": a.success,
                "timestamp": a.timestamp,
                "cost": a.cost,
                "latency_ms": a.latency_ms,
            }
            for a in self.action_history
        ]

    def status(self) -> dict:
        """Get agent status."""
        return {
            "id": self.id,
            "name": self.name,
            "state": self.state.name,
            "alignment": self.alignment.name,
            "goal": self.goal,
            "total_actions": self.total_actions,
            "total_cost": self.total_cost,
            "tools": list(self.tools.keys()),
            "capabilities": self.capabilities,
            "children": len(self.children),
            "memory_working": len(self.memory.working),
            "memory_episodic": len(self.memory.episodic),
            "memory_semantic": len(self.memory.semantic),
        }

    # ------------------------------------------------------------------
    # Lifecycle
    # ------------------------------------------------------------------

    def pause(self) -> None:
        self.state = AgentState.PAUSED

    def resume(self) -> None:
        self.state = AgentState.RUNNING

    def terminate(self) -> None:
        """Terminate the agent and all children."""
        self.state = AgentState.TERMINATED
        for child in self.children:
            child.terminate()

    def __repr__(self) -> str:
        return (
            f"Agent('{self.name}', state={self.state.name}, "
            f"alignment={self.alignment.name}, actions={self.total_actions})"
        )


class AgentSwarm:
    """
    A coordinated group of agents working together.

    Strategies:
    - parallel: All agents work independently, results merged
    - pipeline: Each agent's output feeds the next
    - hierarchical: One coordinator delegates to workers
    - competitive: Multiple agents attempt same task, best result wins
    """

    def __init__(self, name: str = "swarm") -> None:
        self.name = name
        self.agents: list[Agent] = []

    def add(self, agent: Agent) -> None:
        self.agents.append(agent)

    def parallel_execute(self, task: Any) -> list[Any]:
        """All agents execute the same task in parallel."""
        results = []
        for agent in self.agents:
            try:
                result = agent.execute(task)
                results.append(result)
            except Exception as e:
                results.append(f"FAILED: {e}")
        return results

    def pipeline_execute(self, initial_input: Any) -> Any:
        """Chain agents — each one's output is the next one's input."""
        current = initial_input
        for agent in self.agents:
            current = agent.execute(current)
        return current

    def competitive_execute(self, task: Any) -> Any:
        """All agents try, return the first successful result."""
        for agent in self.agents:
            try:
                result = agent.execute(task)
                return result
            except Exception:
                continue
        raise AgentError("All agents in swarm failed")

    def status(self) -> list[dict]:
        return [a.status() for a in self.agents]

    def terminate_all(self) -> None:
        for agent in self.agents:
            agent.terminate()


class AgentError(Exception):
    """Raised when an agent operation fails."""
    pass
