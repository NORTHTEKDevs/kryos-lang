# std::test

A built-in testing framework for writing unit tests, integration tests, and test suites. Provides assertions, lifecycle hooks, structured reporting, and composable test organization.

```kryos
use std::test
```

---

## Types

### TestStatus

```kryos
enum TestStatus {
    Passed,
    Failed(str),
    Skipped(str)
}
```

---

### TestResult

```kryos
struct TestResult {
    name:        str,
    status:      TestStatus,
    elapsed_ms:  f64
}
```

---

### TestReport

```kryos
struct TestReport {
    suite_name:       str,
    results:          [TestResult],
    total:            i64,
    passed:           i64,
    failed:           i64,
    skipped:          i64,
    total_elapsed_ms: f64
}
```

---

### TestCase

```kryos
struct TestCase {
    name:        str,
    body:        fn(),
    skip:        bool,
    skip_reason: str
}
```

---

### TestSuite

```kryos
struct TestSuite {
    name:             str,
    tests:            [TestCase],
    before_each_hook: fn(),
    after_each_hook:  fn(),
    before_all_hook:  fn(),
    after_all_hook:   fn()
}
```

---

## Assertions

### assert

`assert(condition: bool)`

Throw if `condition` is `false`.

---

### assert_eq

`assert_eq(a: any, b: any)`

Throw if `a != b`.

---

### assert_ne

`assert_ne(a: any, b: any)`

Throw if `a == b`.

---

### assert_true

`assert_true(v: bool)`

Throw if `v` is not `true`.

---

### assert_false

`assert_false(v: bool)`

Throw if `v` is not `false`.

---

### assert_null

`assert_null(v: any)`

Throw if `v` is not `null`.

---

### assert_not_null

`assert_not_null(v: any)`

Throw if `v` is `null`.

---

### assert_gt

`assert_gt(a: any, b: any)`

Throw if `a` is not greater than `b`.

---

### assert_lt

`assert_lt(a: any, b: any)`

Throw if `a` is not less than `b`.

---

### assert_gte

`assert_gte(a: any, b: any)`

Throw if `a` is not greater than or equal to `b`.

---

### assert_lte

`assert_lte(a: any, b: any)`

Throw if `a` is not less than or equal to `b`.

---

### assert_approx

`assert_approx(a: f64, b: f64, epsilon: f64)`

Throw if `|a - b| > epsilon`. Use for floating-point comparisons.

---

### assert_contains

`assert_contains(collection: any, item: any)`

Throw if `collection` does not contain `item`. Works with arrays and strings.

---

### assert_throws

`assert_throws(f: fn())`

Throw if `f` does not throw. Passes when `f` raises any error.

---

### assert_no_throw

`assert_no_throw(f: fn())`

Throw if `f` throws. Passes when `f` completes without error.

**Example:**
```kryos
use std::test

assert_eq(1 + 1, 2)
assert_approx(3.14159, 3.14, 0.01)
assert_contains([1, 2, 3], 2)
assert_throws(fn() { let x = 1 / 0 })
```

---

## Defining Tests

### it

`it(name: str, body: fn()) -> TestCase`

Create a test case with the given `name` and `body` function.

---

### xit

`xit(name: str, reason: str) -> TestCase`

Create a skipped test case. The test is recorded as `Skipped` with `reason` and `body` is never called.

**Example:**
```kryos
use std::test

let t1 = it("adds two numbers", fn() {
    assert_eq(1 + 1, 2)
})

let t2 = xit("pending feature", "not implemented yet")
```

---

## Defining Suites

### describe

`describe(name: str, tests: [TestCase]) -> TestSuite`

Create a named test suite containing the given test cases.

---

### before_each

`before_each(suite: TestSuite, hook: fn()) -> TestSuite`

Register a function to call before each test in the suite.

---

### after_each

`after_each(suite: TestSuite, hook: fn()) -> TestSuite`

Register a function to call after each test in the suite.

---

### before_all

`before_all(suite: TestSuite, hook: fn()) -> TestSuite`

Register a function to call once before all tests in the suite.

---

### after_all

`after_all(suite: TestSuite, hook: fn()) -> TestSuite`

Register a function to call once after all tests in the suite.

**Example:**
```kryos
use std::test

let suite = describe("arithmetic", [
    it("addition", fn() { assert_eq(2 + 2, 4) }),
    it("subtraction", fn() { assert_eq(5 - 3, 2) }),
    xit("division", "todo: edge cases")
])
```

---

## Running Tests

### run_test

`run_test(tc: TestCase) -> TestResult`

Execute a single `TestCase` and return its `TestResult`.

---

### run_suite

`run_suite(suite: TestSuite) -> TestReport`

Execute all tests in the suite (respecting lifecycle hooks) and return a `TestReport`.

---

### run_tests

`run_tests(tests: [TestCase]) -> TestReport`

Execute a flat list of test cases and return a `TestReport` (suite name is `"unnamed"`).

---

## Reporting

### format_report

`format_report(report: TestReport) -> str`

Render the report as a formatted string suitable for logging or display.

---

### print_report

`print_report(report: TestReport)`

Print the formatted report to stdout.

---

### all_passed

`all_passed(report: TestReport) -> bool`

Return `true` if no tests in the report failed.

---

### failures

`failures(report: TestReport) -> [TestResult]`

Return only the `TestResult` entries whose status is `Failed`.

---

## Complete Example

```kryos
use std::test

// --- unit tests ---

let math_suite = describe("math", [
    it("addition", fn() {
        assert_eq(1 + 1, 2)
    }),
    it("floating point approx", fn() {
        assert_approx(0.1 + 0.2, 0.3, 1e-9)
    }),
    it("division by zero throws", fn() {
        assert_throws(fn() { let _ = 1 / 0 })
    }),
    xit("modulo edge cases", "tracked in issue #42")
])

// --- lifecycle hooks ---

let db_suite = before_all(
    after_each(
        describe("database", [
            it("insert", fn() { assert_true(true) }),
            it("query",  fn() { assert_true(true) })
        ]),
        fn() { println("teardown after each test") }
    ),
    fn() { println("setup once before all tests") }
)

// --- run and report ---

let report = run_suite(math_suite)
print_report(report)

if !all_passed(report) {
    let bad = failures(report)
    println("failed: " + len(bad))
}
```

---

## Exit Codes

When `print_report` is called, Kryos exits with:

- `0` -- all tests passed or skipped
- `1` -- one or more tests failed
