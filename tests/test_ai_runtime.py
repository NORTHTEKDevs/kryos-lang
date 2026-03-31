"""
Kryos AI-Native Runtime Test Suite

Tests for: Probable, Agents, Streams, Lineage, Cost tracking
"""

import sys
import os

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

passed = 0
failed = 0


def test(name: str, condition: bool, detail: str = ""):
    global passed, failed
    if condition:
        passed += 1
        print(f"  \033[32mPASS\033[0m  {name}")
    else:
        failed += 1
        print(f"  \033[31mFAIL\033[0m  {name}" + (f" — {detail}" if detail else ""))


print("\n" + "=" * 60)
print("Kryos AI-Native Runtime Test Suite")
print("=" * 60)

# ===================================================================
# PROBABLE<T>
# ===================================================================
print("\n[Probable<T> — Probability Type]")

from kryos.runtime.probable import Probable, Ensemble, ProbabilityError

# Basic creation
p = Probable(value="cat", confidence=0.92, alternatives=[("dog", 0.05), ("bird", 0.03)])
test("create Probable with confidence", p.value == "cat" and p.confidence == 0.92)
test("alternatives stored", len(p.alternatives) == 2)

# Certain
c = Probable.certain(42)
test("certain value has 100% confidence", c.confidence == 1.0 and c.value == 42)

# From distribution
d = Probable.uncertain([("A", 0.5), ("B", 0.3), ("C", 0.2)])
test("uncertain picks highest prob", d.value == "A" and d.confidence == 0.5)

# Softmax
s = Probable.from_softmax(["cat", "dog", "bird"], [0.85, 0.10, 0.05])
test("from_softmax works", s.value == "cat" and abs(s.confidence - 0.85) < 0.001)

# Map
mapped = p.map(lambda x: x.upper())
test("map transforms value", mapped.value == "CAT" and mapped.confidence == 0.92)

# Flat map
fm = p.flat_map(lambda x: Probable(value=x + "!", confidence=0.8))
test("flat_map multiplies confidence", abs(fm.confidence - 0.92 * 0.8) < 0.01)

# Threshold
test("is_confident works", p.is_confident(0.9))
test("is_confident rejects low", not p.is_confident(0.95))

# require_confidence
val = p.require_confidence(0.9)
test("require_confidence returns value", val == "cat")

try:
    p.require_confidence(0.99)
    test("require_confidence raises on low", False)
except ProbabilityError:
    test("require_confidence raises on low", True)

# Comparison operators
test("confidence > threshold", p > 0.9)
test("confidence < threshold", p < 0.95)
test("confidence >= threshold", p >= 0.92)

# Bool
test("truthy when > 50%", bool(p) == True)
low = Probable(value="x", confidence=0.3)
test("falsy when < 50%", bool(low) == False)

# Combine
p2 = Probable(value=10, confidence=0.8)
p3 = Probable(value=20, confidence=0.9)
combined = p2.combine(p3, lambda a, b: a + b)
test("combine values", combined.value == 30)
test("combine confidence = product", abs(combined.confidence - 0.72) < 0.01)

# Entropy
test("entropy is non-negative", p.entropy() >= 0)

# Explain
explanation = p.explain()
test("explain returns string", "cat" in explanation and "0.92" in explanation or "92" in explanation)

# Ensemble
preds = [
    Probable(value="A", confidence=0.8),
    Probable(value="B", confidence=0.6),
    Probable(value="A", confidence=0.7),
    Probable(value="A", confidence=0.9),
]
voted = Ensemble.majority_vote(preds)
test("majority_vote picks most common", voted.value == "A")
test("majority_vote confidence is ratio", voted.confidence == 0.75)

weighted = Ensemble.weighted_average(preds)
test("weighted_average returns Probable", isinstance(weighted, Probable))

best = Ensemble.best_confidence(preds)
test("best_confidence picks highest", best.confidence == 0.9)

# ===================================================================
# AGENTS
# ===================================================================
print("\n[Agents — Autonomous Agent Runtime]")

from kryos.runtime.agents import Agent, AgentSwarm, AlignmentMode, AgentError

# Create unrestricted agent
agent = Agent(name="ReconBot", goal="scan everything", alignment=AlignmentMode.UNRESTRICTED)
test("create unrestricted agent", agent.alignment == AlignmentMode.UNRESTRICTED)
test("agent has name", agent.name == "ReconBot")

# Add tools
agent.add_tool("multiply", lambda x, y: x * y, "Multiply two numbers")
result = agent.use_tool("multiply", 6, 7)
test("agent uses tool", result == 42)
test("action recorded", agent.total_actions == 1)

# Memory
agent.memory.remember("target", "192.168.1.0/24", "working")
agent.memory.remember("learned_patterns", ["syn_scan", "stealth"], "semantic")
test("working memory", agent.memory.recall("target") == "192.168.1.0/24")
test("semantic memory", agent.memory.recall("learned_patterns") == ["syn_scan", "stealth"])

# Execute
result = agent.execute("test task")
test("agent executes", "completed" in result.lower() or "ReconBot" in result)

# Child agent
child = agent.spawn_child("SubBot", goal="sub task")
test("spawn child inherits alignment", child.alignment == AlignmentMode.UNRESTRICTED)

# Child cannot exceed parent capabilities
cap_agent = Agent(name="Limited", capabilities=["compute", "network"])
child2 = cap_agent.spawn_child("Sub", capabilities=["compute"])
test("child narrows capabilities", child2.capabilities == ["compute"])

try:
    cap_agent.spawn_child("Bad", capabilities=["compute", "quantum"])
    test("child cannot exceed parent caps", False)
except AgentError:
    test("child cannot exceed parent caps", True)

# Swarm
swarm = AgentSwarm("test_swarm")
swarm.add(Agent(name="Worker1"))
swarm.add(Agent(name="Worker2"))
results = swarm.parallel_execute("do work")
test("swarm parallel execution", len(results) == 2)

# Audit trail
trail = agent.get_audit_trail()
test("audit trail is list", isinstance(trail, list) and len(trail) > 0)

# Status
status = agent.status()
test("status has all fields", "name" in status and "alignment" in status)
test("alignment is UNRESTRICTED", status["alignment"] == "UNRESTRICTED")

# Terminate
agent.terminate()
test("terminate agent", agent.state.name == "TERMINATED")

# ===================================================================
# STREAMS
# ===================================================================
print("\n[Streams — Reactive Data Processing]")

from kryos.runtime.streams import Stream

# From list
s = Stream.from_list([1, 2, 3, 4, 5])
test("collect stream", s.collect() == [1, 2, 3, 4, 5])

# Map
s2 = Stream.from_list([1, 2, 3]).map(lambda x: x * 2)
test("stream map", s2.collect() == [2, 4, 6])

# Filter
s3 = Stream.from_list([1, 2, 3, 4, 5]).filter(lambda x: x > 3)
test("stream filter", s3.collect() == [4, 5])

# Chain operations
result = (Stream.from_list(range(10))
    .filter(lambda x: x % 2 == 0)
    .map(lambda x: x ** 2)
    .take(3)
    .collect())
test("chained stream ops", result == [0, 4, 16])

# Batch
batches = Stream.from_list(range(10)).batch(3).collect()
test("stream batch", batches == [[0, 1, 2], [3, 4, 5], [6, 7, 8], [9]])

# Window
windows = Stream.from_list([1, 2, 3, 4, 5]).window(3).collect()
test("sliding window", windows == [[1, 2, 3], [2, 3, 4], [3, 4, 5]])

# Reduce
total = Stream.from_list([1, 2, 3, 4, 5]).reduce(lambda a, b: a + b)
test("stream reduce", total == 15)

# Scan
running = Stream.from_list([1, 2, 3, 4]).scan(lambda acc, x: acc + x, 0).collect()
test("stream scan", running == [1, 3, 6, 10])

# First/Last
test("stream first", Stream.from_list([10, 20, 30]).first() == 10)
test("stream last", Stream.from_list([10, 20, 30]).last() == 30)

# Sum/Min/Max
test("stream sum", Stream.from_list([1, 2, 3]).sum() == 6)
test("stream min", Stream.from_list([3, 1, 2]).min() == 1)
test("stream max", Stream.from_list([3, 1, 2]).max() == 3)

# Any/All
test("stream any", Stream.from_list([1, 2, 3]).any(lambda x: x > 2))
test("stream all", Stream.from_list([2, 4, 6]).all(lambda x: x % 2 == 0))

# Merge
merged = Stream.merge(
    Stream.from_list([1, 3, 5]),
    Stream.from_list([2, 4, 6]),
).collect()
test("stream merge interleaves", len(merged) == 6)

# Concat
concatenated = Stream.concat(
    Stream.from_list([1, 2]),
    Stream.from_list([3, 4]),
).collect()
test("stream concat", concatenated == [1, 2, 3, 4])

# Zip
zipped = Stream.zip(
    Stream.from_list([1, 2, 3]),
    Stream.from_list(["a", "b", "c"]),
).collect()
test("stream zip", zipped == [(1, "a"), (2, "b"), (3, "c")])

# Count
test("stream count", Stream.from_list([1, 2, 3, 4, 5]).count() == 5)

# ===================================================================
# LINEAGE
# ===================================================================
print("\n[Lineage — Data Provenance Tracking]")

from kryos.runtime.lineage import Tracked

# Create tracked data
data = Tracked.source([1, 2, 3, 4, 5], source="/data/input.csv")
test("tracked source", data.value == [1, 2, 3, 4, 5])
test("lineage has source step", len(data.lineage) == 1 and data.lineage[0].operation == "source")

# Transform
transformed = data.transform(lambda x: [v * 2 for v in x], description="double all values")
test("tracked transform", transformed.value == [2, 4, 6, 8, 10])
test("lineage grows", len(transformed.lineage) == 2)

# Filter
filtered = data.filter(lambda x: x > 2, description="keep > 2")
test("tracked filter", filtered.value == [3, 4, 5])
test("filter lineage records count", filtered.lineage[-1].operation == "filter")

# Inference annotation
result = data.inference("model_v3", result="anomaly_detected", confidence=0.95)
test("inference step recorded", result.lineage[-1].operation == "inference")
test("inference metadata", result.lineage[-1].metadata["confidence"] == 0.95)

# Explain
explanation = transformed.explain()
test("explain is readable", "double" in explanation)

# JSON export
json_str = data.to_json()
test("JSON export works", "/data/input.csv" in json_str)

# ===================================================================
# COST TRACKING
# ===================================================================
print("\n[Cost — Computation Cost Tracking]")

from kryos.runtime.cost import ComputeCost, Budget, CostTracker, BudgetExceeded

# Basic cost
cost1 = ComputeCost(tokens_used=1000, money_usd=0.003)
cost2 = ComputeCost(tokens_used=500, money_usd=0.001)
total = cost1 + cost2
test("cost addition", total.tokens_used == 1500 and abs(total.money_usd - 0.004) < 0.0001)

# Budget
budget = Budget(max_usd=0.01, max_tokens=5000)
budget.charge(cost1)
test("budget tracks spending", budget.spent.tokens_used == 1000)
test("budget not exhausted", not budget.is_exhausted)

# Budget exceeded
try:
    budget.charge(ComputeCost(money_usd=1.0))
    test("budget exceeded raises", False)
except BudgetExceeded:
    test("budget exceeded raises", True)

# Cost tracker
tracker = CostTracker()
with tracker:
    # Simulate work
    _ = sum(range(10000))
test("tracker measures wall time", tracker.total.wall_time_ms > 0)

tracker.record_tokens(500, cost_per_token=0.00001)
test("tracker records tokens", tracker.total.tokens_used == 500)

# ===================================================================
# INTEGRATION — Kryos interpreter with AI runtime
# ===================================================================
print("\n[Integration — Interpreter + AI Runtime]")

from kryos.compiler.lexer import tokenize
from kryos.compiler.parser import parse
from kryos.compiler.interpreter import Interpreter

# Test Probable from Kryos code
source = """
let p = Probable("cat", 0.92)
println(type_of(p))
"""
tokens = tokenize(source, "<test>")
module = parse(tokens)
interp = Interpreter(output_fn=lambda s: None)
interp.run(module)
test("Probable accessible from Kryos", len(interp.output) > 0)

# Test Agent from Kryos code
source2 = """
let bot = Agent("TestBot", "do stuff", "unrestricted")
println(type_of(bot))
"""
tokens2 = tokenize(source2, "<test>")
module2 = parse(tokens2)
interp2 = Interpreter(output_fn=lambda s: None)
interp2.run(module2)
test("Agent accessible from Kryos", len(interp2.output) > 0)

# Test Stream from Kryos code
source3 = """
let s = Stream([1, 2, 3, 4, 5])
println(type_of(s))
"""
tokens3 = tokenize(source3, "<test>")
module3 = parse(tokens3)
interp3 = Interpreter(output_fn=lambda s: None)
interp3.run(module3)
test("Stream accessible from Kryos", len(interp3.output) > 0)

# ===================================================================
# Results
# ===================================================================
print("\n" + "=" * 60)
total_tests = passed + failed
print(f"Results: {passed}/{total_tests} passed, {failed} failed")
print("=" * 60)

sys.exit(0 if failed == 0 else 1)
