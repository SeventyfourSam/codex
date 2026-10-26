# Token output speed

Add `token-output-speed` through `/statusline`, or append it to your existing
`[tui].status_line` list. It displays the **last completed model request's average**,
for example `42.0 tk/s`. The item is optional and hidden until a sample is available.
Only ordinary embedded local sessions are supported. Local daemon and remote
connections hide this item because they do not share the TUI process's memory.

The calculation is the provider's actual `output_tokens` divided by monotonic elapsed
seconds from request preparation to receipt of `ResponseEvent::Completed`. Output
tokens already include reasoning tokens; reasoning is not added a second time.
This is a client-observed request average, including request transmission, first-token
wait, and generation, not the server's internal decode-only speed.

Each request has a separate timer. Tool execution and approval waits between requests
are excluded. The last completed value stays visible while the next request or a tool
runs. Each completion replaces that thread's sample, and the TUI reads it on its
normal status-line refresh. Display can therefore update after tools finish, but
tool time is never included in the measurement. It does not combine other threads
or subagents. No live token estimate is displayed.
Warmups, failed requests, and cancelled requests do not produce samples. A completed
request without usage clears the value. Samples are not persisted, and the cache
retains at most 128 threads.

Implementation uses the official `ModelRequestContributor` / `ModelResponseInterceptor`
extension interface in an app-server custom module. Events and errors pass through
unchanged. An independent module in the already-shared `codex-otel` runtime crate
holds completed samples in process memory. These values are not exported as
telemetry. No protocol notifications, disk writes, polling, or additional provider
requests are added. Timing is not added to upstream usage structs.

Official-source changes are limited to module registration, contributor registration,
and status-line item enumeration, preview, styling, and rendering: seven upstream
files. Core inference/retry code, protocol definitions, and generated SDK artifacts
are unchanged.
