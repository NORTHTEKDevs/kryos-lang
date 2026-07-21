# Call an LLM (OpenAI-compatible or Anthropic)

`std::llm` speaks both major wire formats over the native HTTPS client.
The same code works against OpenAI, Anthropic, OpenRouter, or a local
Ollama/vLLM server (point `with_base_url` at it).

```kryos
use std::llm::{anthropic_config, openai_config, with_base_url, system, user, chat, complete}

@capabilities(process, net)
fn main() {
    // One-shot ask against Anthropic. (`complete`, because `ask` is a
    // reserved keyword.)
    let claude = anthropic_config(env_get("ANTHROPIC_API_KEY"), "claude-sonnet-4-6")
    let reply = complete(claude, "Say hello in exactly five words.")
    println(reply)

    // Multi-turn chat against a local Ollama server -- no API key needed.
    let local = with_base_url(openai_config("", "llama3"), "http://127.0.0.1:11434/v1")
    let r = chat(local, [
        system("You are terse."),
        user("What is 2 + 2?")
    ])
    println("{r.text}  ({r.input_tokens} tokens in, {r.output_tokens} out)")
}
```

Errors `throw` with an `llm error: ...` message — catch them or let the
program exit 101 with the message.

## Budget enforcement

`chat_within` guards a call with a `std::cost` budget: it throws **before**
the request when the budget is exhausted, and charges actual token usage
plus one API call afterward.

```kryos
use std::llm::{anthropic_config, user, chat_within}
use std::cost::{Budget, ComputeCost}

@capabilities(process, net)
fn main() {
    let cfg = anthropic_config(env_get("ANTHROPIC_API_KEY"), "claude-sonnet-4-6")
    let mut budget = Budget { max_usd: 1.0, max_tokens: 50000, max_api_calls: 25, spent_usd: 0.0, spent_tokens: 0, spent_api_calls: 0 }

    let out = chat_within(cfg, [user("Summarize ownership in Kryos in two sentences.")], budget)
    budget = out.budget
    println(out.response.text)
    println("spent so far: {budget.spent_tokens} tokens, {budget.spent_api_calls} calls")
}
```

Dollar accounting is left at 0.0 by design — per-token pricing varies by
provider and model; `charge()` a `ComputeCost` with your own rates if you
track spend in USD.

## The `@budget` attribute — hard ceilings, enforced by the compiler

For agent loops, prefer the function attribute: it pushes a thread-local
budget frame on entry, pops it on every return, and every `std::llm` call
inside (at any depth) charges against it automatically. Exceeding the
ceiling throws, which is what halts a runaway loop. Nested `@budget`
functions stack — an outer budget constrains everything inside it.

<!-- docs-example: skip -->
```kryos
use std::llm::{anthropic_config, user, chat}

@budget(tokens = 50000, calls = 25)
@capabilities(process, net)
fn research_agent(question: str) -> str {
    let cfg = anthropic_config(env_get("ANTHROPIC_API_KEY"), "claude-sonnet-4-6")
    let mut notes = ""
    let mut round = 0
    while round < 100 {
        // The 26th call throws "llm error: @budget exhausted" no matter
        // what the loop condition says.
        let r = chat(cfg, [user(question + "
Notes so far: " + notes)])
        notes = notes + "
" + r.text
        round = round + 1
    }
    return notes
}
```

Omit an axis for no limit on it: `@budget(calls = 10)` caps calls only.
The token ceiling is charged after each response (real usage), so the call
that crosses the line completes but the next loop iteration never starts.
