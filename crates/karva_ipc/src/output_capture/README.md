# Output capture measurements

The default `max-output-bytes` is 16 MiB per stream and attempt. Buffers allocate
only as bytes arrive and retain equal leading and trailing portions after the
budget is exhausted. Pipe readers use the standard library's `BufReader` default.

On 2026-10-12, a Python 3.13 integration-test workload emitting 1,000 structured
request logs produced 202,890 bytes. A child process read and echoed those bytes
in 0.016 seconds. Each record contained level, request ID, HTTP method and path,
status, message, customer and currency fields. The default provides about 82
such workloads per stream. The six existing captured-stdout snapshot sections
were at most 214 bytes, including the diagnostic text following each section.

Raise the configured budget when real workloads need more output; omitted bytes
always appear in the retained output with the budget name and requested total.

Normal attempts pipeline their output with existing completion frames; only
deadline and scoped-fixture collection wait for replies. Completed windows use
the configured per-stream budget as a backpressure threshold, counting record
sizes and allocated byte capacities. A window already in progress can finish
before the next begin marker blocks. Empty windows therefore queue without
allocating output buffers or requiring a reader handoff per attempt. Worker
shutdown has one drain barrier so queued completions precede stream teardown.

On 2026-10-14, release builds on Apple Silicon with Python 3.13.14 reproduced
three benchmark-generator workloads with default workers and live status.
Each revision ran 13 times in rotating order; medians exclude its first warmup.
The parent collected CLI stdout and stderr, and no builds ran during timing.
Main (`1a8a9d83`), the original capture PR (`b50dab11`), and pipelined capture
respectively took 0.330, 0.635, and 0.369 seconds for 8,000 empty functions;
0.657, 1.253, and 0.655 seconds for 16,807 matrix variants; and 0.381, 0.581,
and 0.416 seconds for 16,384 tests using yield fixtures. Relative to the original
PR this reduces wall time by about 42%, 48%, and 28%. The empty-function and
fixture probes still show about 12% and 9% overhead against main, including
native boundary writes and descriptor restoration. These local wall times are
noisy and are not a substitute for the CI benchmark comparison.
