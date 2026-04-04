# Kryos Standard Library

The Kryos standard library ships with every installation. Core builtins are available in every `.kry` program without imports. Stdlib modules are auto-registered at interpreter startup and their functions are globally accessible.

## Module Index

| Module | Description |
|--------|-------------|
| [Core Builtins](core-builtins.md) | Always-available functions: I/O, math, strings, arrays, conversion, assert |
| `std::string` | String manipulation utilities (`upper`, `lower`, `trim`, `split`, `join`, `replace`, `contains`, `starts_with`, `ends_with`, `substr`, `char_at`, `char_code`, `char_from`) |
| `std::string_ext` | Extended string operations (`string_repeat`, `string_pad_left`, `string_pad_right`, `string_lines`, `string_index`, `string_count`, `to_int`, `to_float`) |
| `std::math` | Extended math functions (`round`, `log10`, `random`, `pi`, `e`) beyond core (`sin`, `cos`, `tan`, `log`, `pow`, `sqrt`, `floor`, `ceil`, `min`, `max`, `abs`) |
| `std::collections` | Higher-order collection functions (`map`, `filter`, `reduce`, `sort`, `reverse`, `zip`, `enumerate`, `find`, `any`, `all`, `flat_map`, `sum`, `count`) |
| `std::map` | Hash map operations (`map_new`, `map_set`, `map_get`, `map_has`, `map_keys`, `map_values`, `map_remove`, `map_merge`, `map_from`) |
| `std::set` | Hash set operations (`set_new`, `set_add`, `set_remove`, `set_has`, `set_size`, `set_union`, `set_intersection`, `set_difference`, `set_to_array`, `set_from_array`) |
| `std::json` | JSON parsing and serialization (`json_parse`, `json_stringify`, `json_get`, `json_has`) |
| `std::io` | File system, paths, environment, temp files (`file_read`, `file_write`, `file_append`, `file_exists`, `file_delete`, `dir_list`, `dir_create`, `glob`, `path_join`, `stdin_read`, `env_get`, `cwd`, `temp_file`) |
| `std::net` | HTTP client, WebSocket, TCP, URL encoding (`http_get`, `http_post`, `http_request`, `http_get_json`, `http_post_json`, `ws_connect`, `tcp_connect`, `url_encode`, `url_decode`) |
| `std::crypto` | Hashing, HMAC, encoding, random bytes, UUIDs (`sha256`, `sha512`, `md5`, `hmac_sha256`, `base64_encode`, `base64_decode`, `hex_encode`, `hex_decode`, `random_bytes`, `uuid`) |
| `std::regex` | Regular expressions (`regex_match`, `regex_search`, `regex_find_all`, `regex_replace`, `regex_split`, `regex_test`, `regex_escape`) |
| `std::datetime` | Date and time (`now`, `now_ms`, `now_iso`, `now_local`, `timestamp_to_iso`, `iso_to_timestamp`, `format_date`, `parse_date`, `date_add`, `date_diff`, `date_parts`, `duration`) |
| `std::term` | Terminal control (`term_clear`, `term_write`, `term_move`, `term_size`, `term_color`, `term_bold`, `term_rgb`, `term_read_key`, `term_raw_mode`) |
| `std::process` | Process execution (`exec`, `exec_capture`, `exec_timeout`) |
| `std::server` | HTTP server framework (`http_app`, `app_get`, `app_post`, `app_put`, `app_delete`, `app_use`, `app_listen`, `respond`, `cors_middleware`, `rate_limit`) |
| `std::db` | Database connectivity (`db_connect`, `db_query`, `db_query_one`, `db_execute`, `db_close`) |
| `std::auth` | Authentication utilities (`jwt_sign`, `jwt_verify`, `jwt_decode`, `hash_password`, `verify_password`, `generate_token`) |
| `std::config` | Configuration and environment (`env_load`, `env_require`, `env_default`, `env_all`, `config`) |
| `std::email` | Email sending (`send_email`, `send_email_resend`) |
| `std::claude` | Anthropic Claude API integration (`claude_message`, `claude_messages`, `claude_json`, `claude_embed`) |
| `std::stripe` | Stripe payments integration (`stripe_create_customer`, `stripe_create_checkout`, `stripe_create_subscription`, `stripe_verify_webhook`) |

## Architecture

All stdlib modules are registered at interpreter startup by `register_all_stdlib()` in `kryos/stdlib/__init__.py`. Every function is injected into the global environment, making it callable from any `.kry` file without an explicit `use` statement.

Core builtins (I/O, math, strings, arrays, conversion, assert) are registered directly by the interpreter's `_setup_builtins()` method. Stdlib modules add additional domain-specific functionality on top of these.

## AI-Native Runtime

In addition to the standard library, Kryos ships with first-class AI runtime primitives registered as globals:

- `Probable(value, confidence)` -- probability-weighted values
- `Agent(name, goal, alignment)` -- autonomous agent creation
- `Stream(data)` -- lazy data streams
- `Tracked(value, source)` -- lineage tracking
- `Tensor(data)` -- multi-dimensional numeric arrays (GPU-accelerable)
- `Budget(max_usd, max_tokens)` / `CostTracker(budget)` -- compute cost tracking

## FFI (Foreign Function Interface)

Python and C interop is available as global builtins:

- `py_import(module)`, `py_call(module, fn, ...)`, `py_attr(module, attr)` -- Python FFI
- `c_load(path)`, `c_call(lib, fn, arg_types, return_type, ...)` -- Native C FFI
