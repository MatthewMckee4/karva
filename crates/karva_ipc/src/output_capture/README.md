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

On 2026-10-13, nine alternating debug-build runs of 2,000 empty tests on Apple
Silicon with Python 3.13 had median wall times of 0.223 seconds on main (`6b19a776`) and
0.322 seconds with controller-owned capture (about 50 microseconds more per
test). Each run used one worker with live and final status output disabled.
The first run of each build was cold; medians include it. This measures the
per-attempt descriptor restoration and synchronous output collection cost,
not throughput of tests that do useful work.
