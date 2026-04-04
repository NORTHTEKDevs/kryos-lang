# std::claude

Anthropic Claude API integration for AI-powered applications. Uses the Messages API directly via `urllib` -- no external SDK required. Requires the `ANTHROPIC_API_KEY` environment variable.

Default model: `claude-sonnet-4-20250514`. Default max tokens: 4096.

```kryos
import std::claude
```

---

### claude_message

`claude_message(prompt: String) -> String`
`claude_message(prompt: String, options: Map) -> String`

Send a single message to Claude and get a text response.

**Options map:**
| Key | Default | Description |
|-----|---------|-------------|
| `model` | `claude-sonnet-4-20250514` | Model ID |
| `max_tokens` | `4096` | Maximum response tokens |
| `system` | none | System prompt |
| `temperature` | none | Sampling temperature (0.0 - 1.0) |

**Example:**
```kryos
let answer = claude_message("What is the capital of Alaska?")
print(answer)  // Juneau
```

```kryos
let summary = claude_message("Summarize this in 2 sentences: " + article, map_from(
    "model", "claude-haiku-4-5-20251001",
    "system", "You are a concise technical writer.",
    "max_tokens", 200
))
```

**Edge cases:**
- Raises if `ANTHROPIC_API_KEY` is not set.
- Raises on API errors (includes the first 500 chars of the error body).
- Request timeout is 120 seconds.

**See also:** claude_messages, claude_json

---

### claude_messages

`claude_messages(messages: Array) -> String`
`claude_messages(messages: Array, options: Map) -> String`

Send a multi-turn conversation to Claude. Each message is a map with `role` and `content` fields. Accepts the same options as `claude_message`.

**Example:**
```kryos
let history = [
    map_from("role", "user", "content", "My name is Alice."),
    map_from("role", "assistant", "content", "Hello Alice! How can I help you?"),
    map_from("role", "user", "content", "What is my name?")
]
let answer = claude_messages(history)
print(answer)  // Your name is Alice.
```

```kryos
// With system prompt and model override
let answer = claude_messages(history, map_from(
    "system", "You are a helpful coding assistant.",
    "model", "claude-sonnet-4-20250514"
))
```

**Edge cases:**
- Raises if the first argument is not an array.
- Messages must alternate between `user` and `assistant` roles per the API contract.

**See also:** claude_message

---

### claude_json

`claude_json(prompt: String) -> Map | Array`
`claude_json(prompt: String, options: Map) -> Map | Array`

Send a message to Claude and parse the response as JSON. Useful for structured data extraction. Automatically adds a JSON instruction to the system prompt.

**Example:**
```kryos
let data = claude_json("Extract the name, age, and city from this text: 'Alice is 30 and lives in Juneau.'")
print(data.name)  // Alice
print(data.age)   // 30
print(data.city)  // Juneau
```

```kryos
let tasks = claude_json(
    "Generate 3 sample todo items with title and priority (high/med/low)",
    map_from("model", "claude-haiku-4-5-20251001")
)
for task in tasks {
    print(task.title + " [" + task.priority + "]")
}
```

**Edge cases:**
- Automatically strips markdown code fences from the response before parsing.
- Raises if the response is not valid JSON.
- Accepts all the same options as `claude_message` (the `system` option is appended to, not replaced).

**See also:** claude_message

---

### claude_embed

`claude_embed(text: String) -> Array`

Generate an embedding vector for text. Returns an array of 8 float values between -1.0 and 1.0.

**Note:** This is currently a deterministic hash-based placeholder. Anthropic does not yet offer a native embeddings endpoint. For production semantic search, use a dedicated embedding model.

**Example:**
```kryos
let vec = claude_embed("machine learning")
print(len(vec))  // 8
print(vec[0])    // -0.234... (deterministic for the same input)
```

**Edge cases:**
- The same input always produces the same output (deterministic).
- Not suitable for real semantic similarity -- use for prototyping only.

**See also:** claude_message
