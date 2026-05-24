<!-- WARNING: This file is auto-generated (cargo run -p karva_dev generate-all). Edit the doc comments in 'crates/karva/src/args.rs' if you want to change anything here. -->

# CLI Reference

## karva

A Python test runner.

<h3 class="cli-reference">Usage</h3>

```
karva <COMMAND>
```

<h3 class="cli-reference">Commands</h3>

<dl class="cli-reference"><dt><a href="#karva-test"><code>karva test</code></a></dt><dd><p>Run tests</p></dd>
<dt><a href="#karva-snapshot"><code>karva snapshot</code></a></dt><dd><p>Manage snapshots created by <code>karva.assert_snapshot()</code></p></dd>
<dt><a href="#karva-cache"><code>karva cache</code></a></dt><dd><p>Manage the karva cache</p></dd>
<dt><a href="#karva-version"><code>karva version</code></a></dt><dd><p>Display Karva's version</p></dd>
<dt><a href="#karva-help"><code>karva help</code></a></dt><dd><p>Print this message or the help of the given subcommand(s)</p></dd>
</dl>

## karva test

Run tests

<h3 class="cli-reference">Usage</h3>

```
karva test [OPTIONS] [PATH]...
```

<h3 class="cli-reference">Arguments</h3>

<dl class="cli-reference"><dt id="karva-test--paths"><a href="#karva-test--paths"><code>PATHS</code></a></dt><dd><p>List of files, directories, or test functions to test &#91;default: the project root&#93;</p>
</dd></dl>

<h3 class="cli-reference">Options</h3>

<dl class="cli-reference"><dt id="karva-test--color"><a href="#karva-test--color"><code>--color</code></a> <i>color</i></dt><dd><p>Control when colored output is used</p>
<p>Possible values:</p>
<ul>
<li><code>auto</code>:  Display colors if the output goes to an interactive terminal</li>
<li><code>always</code>:  Always display colors</li>
<li><code>never</code>:  Never display colors</li>
</ul></dd><dt id="karva-test--config-file"><a href="#karva-test--config-file"><code>--config-file</code></a> <i>path</i></dt><dd><p>The path to a <code>karva.toml</code> file to use for configuration.</p>
<p>While karva configuration can be included in a <code>pyproject.toml</code> file, it is not allowed in this context.</p>
<p>May also be set with the <code>KARVA_CONFIG_FILE</code> environment variable.</p></dd><dt id="karva-test--cov"><a href="#karva-test--cov"><code>--cov</code></a> <i>source</i></dt><dd><p>Measure code coverage for the given source path.</p>
<p>May be passed multiple times to measure several sources. Pass without a value (<code>--cov</code>) to measure the current working directory.</p>
</dd><dt id="karva-test--cov-fail-under"><a href="#karva-test--cov-fail-under"><code>--cov-fail-under</code></a> <i>percent</i></dt><dd><p>Fail the run if total coverage is below the given percentage.</p>
<p>Accepts any value in <code>0..=100</code> (fractional values such as <code>90.5</code> are allowed). When the reported <code>TOTAL</code> percentage is below the threshold, the test command exits with a non-zero status even if every test passed. Has no effect when tests have already failed.</p>
</dd><dt id="karva-test--cov-report"><a href="#karva-test--cov-report"><code>--cov-report</code></a> <i>type</i></dt><dd><p>Coverage terminal report type.</p>
<p><code>term</code> (default) prints a compact terminal table. <code>term-missing</code> extends it with a <code>Missing</code> column listing the uncovered line numbers per file.</p>
<p>Possible values:</p>
<ul>
<li><code>term</code>:  Compact terminal table (default)</li>
<li><code>term-missing</code>:  Terminal table with a <code>Missing</code> column listing uncovered line numbers</li>
</ul></dd><dt id="karva-test--durations"><a href="#karva-test--durations"><code>--durations</code></a> <i>n</i></dt><dd><p>Show the N slowest tests after the run completes</p>
</dd><dt id="karva-test--fail-fast"><a href="#karva-test--fail-fast"><code>--fail-fast</code></a></dt><dd><p>Stop scheduling new tests after the first failure.</p>
<p>Equivalent to <code>--max-fail=1</code>. Use <code>--no-fail-fast</code> to keep running after failures.</p>
</dd><dt id="karva-test--filter"><a href="#karva-test--filter"><code>--filter</code></a>, <code>-E</code> <i>filter-expressions</i></dt><dd><p>Filter tests using a filterset expression.</p>
<p>Predicates: <code>test(&lt;matcher&gt;)</code> matches the fully qualified test name; <code>tag(&lt;matcher&gt;)</code> matches any custom tag on the test.</p>
<p>Matchers: <code>=exact</code>, <code>~substring</code>, <code>/regex/</code>, <code>#glob</code>. The default is substring for <code>test()</code> and exact for <code>tag()</code>. String bodies may be quoted (<code>&quot;...&quot;</code>) to allow spaces or reserved characters.</p>
<p>Operators: <code>&amp;</code> / <code>and</code>, <code>|</code> / <code>or</code>, <code>not</code> / <code>!</code>, and <code>-</code> as shorthand for &quot;and not&quot;. Use parentheses for grouping. <code>and</code> binds tighter than <code>or</code>.</p>
<p>When specified multiple times, a test runs if it matches any of the expressions (OR semantics across flags).</p>
<p>Examples: <code>-E 'tag(slow)'</code>, <code>-E 'test(/^mod::test_login$/)'</code>, <code>-E 'tag(slow) &amp; test(~login)'</code>, <code>-E '(tag(fast) | tag(unit)) - tag(flaky)'</code>.</p>
</dd><dt id="karva-test--final-status-level"><a href="#karva-test--final-status-level"><code>--final-status-level</code></a> <i>level</i></dt><dd><p>Test summary information to display at the end of the run &#91;default: pass&#93;</p>
<p>May also be set with the <code>KARVA_FINAL_STATUS_LEVEL</code> environment variable.</p><p>Possible values:</p>
<ul>
<li><code>none</code>:  Don't display the summary line or any diagnostic blocks</li>
<li><code>fail</code>:  Only display the summary line and diagnostics on failure</li>
<li><code>retry</code>:  Display the summary line plus diagnostics on failure or when any test was retried. The summary line gains a <code>N retried</code> count whenever a retry happened</li>
<li><code>slow</code>:  Same as <code>retry</code> until a slow-test threshold is implemented</li>
<li><code>pass</code>:  Always display the summary line and diagnostics (default)</li>
<li><code>skip</code>:  Same as <code>pass</code> until skip-specific summary lines are emitted</li>
<li><code>all</code>:  Always display every summary status</li>
</ul></dd><dt id="karva-test--help"><a href="#karva-test--help"><code>--help</code></a>, <code>-h</code></dt><dd><p>Print help (see a summary with '-h')</p>
</dd><dt id="karva-test--last-failed"><a href="#karva-test--last-failed"><code>--last-failed</code></a>, <code>--lf</code></dt><dd><p>Re-run only the tests that failed in the previous run</p>
</dd><dt id="karva-test--max-fail"><a href="#karva-test--max-fail"><code>--max-fail</code></a> <i>n</i></dt><dd><p>Stop scheduling new tests after this many failures.</p>
<p>Accepts a positive integer such as <code>--max-fail=3</code>. <code>--max-fail=1</code> is equivalent to the legacy <code>--fail-fast</code>, and <code>--no-fail-fast</code> clears the limit. When <code>--max-fail</code> is provided alongside <code>--fail-fast</code> or <code>--no-fail-fast</code>, <code>--max-fail</code> takes precedence.</p>
</dd><dt id="karva-test--no-cache"><a href="#karva-test--no-cache"><code>--no-cache</code></a></dt><dd><p>Disable reading the karva cache for test duration history</p>
</dd><dt id="karva-test--no-capture"><a href="#karva-test--no-capture"><code>--no-capture</code></a></dt><dd><p>Disable output capture and run tests serially.</p>
<p>Lets stdout/stderr from tests flow directly to the terminal, useful when debugging with print statements or interactive debuggers. Implies <code>--show-output</code> and forces a single worker so output from concurrent tests cannot interleave.</p>
</dd><dt id="karva-test--no-cov"><a href="#karva-test--no-cov"><code>--no-cov</code></a></dt><dd><p>Disable coverage measurement for this run.</p>
<p>Overrides any <code>--cov</code> flag and any <code>&#91;coverage&#93; sources</code> configured in <code>karva.toml</code> / <code>pyproject.toml</code>. Useful when iterating locally without editing config.</p>
</dd><dt id="karva-test--no-fail-fast"><a href="#karva-test--no-fail-fast"><code>--no-fail-fast</code></a></dt><dd><p>Run every test regardless of how many fail.</p>
<p>Clears any <code>fail-fast</code> or <code>max-fail</code> value set in configuration. When <code>--max-fail</code> is provided alongside <code>--no-fail-fast</code>, <code>--max-fail</code> takes precedence.</p>
</dd><dt id="karva-test--no-ignore"><a href="#karva-test--no-ignore"><code>--no-ignore</code></a></dt><dd><p>When set, .gitignore files will not be respected</p>
</dd><dt id="karva-test--no-parallel"><a href="#karva-test--no-parallel"><code>--no-parallel</code></a></dt><dd><p>Disable parallel execution (equivalent to <code>--num-workers 1</code>)</p>
</dd><dt id="karva-test--no-tests"><a href="#karva-test--no-tests"><code>--no-tests</code></a> <i>action</i></dt><dd><p>Behavior when no tests are found to run &#91;default: auto&#93;</p>
<p>May also be set with the <code>KARVA_NO_TESTS</code> environment variable.</p><p>Possible values:</p>
<ul>
<li><code>auto</code>:  Automatically determine behavior: fail if no filter expressions were given, pass silently if filters were given</li>
<li><code>pass</code>:  Silently exit with code 0</li>
<li><code>warn</code>:  Produce a warning and exit with code 0</li>
<li><code>fail</code>:  Produce an error message and exit with a non-zero code</li>
</ul></dd><dt id="karva-test--num-workers"><a href="#karva-test--num-workers"><code>--num-workers</code></a>, <code>-n</code> <i>num-workers</i></dt><dd><p>Number of parallel workers (default: number of CPU cores)</p>
</dd><dt id="karva-test--output-format"><a href="#karva-test--output-format"><code>--output-format</code></a> <i>output-format</i></dt><dd><p>The format to use for printing diagnostic messages</p>
<p>Possible values:</p>
<ul>
<li><code>full</code>:  Print diagnostics verbosely, with context and helpful hints (default)</li>
<li><code>concise</code>:  Print diagnostics concisely, one per line</li>
</ul></dd><dt id="karva-test--partition"><a href="#karva-test--partition"><code>--partition</code></a> <i>strategy:m/n</i></dt><dd><p>Run only a slice of the collected tests, distributed round-robin.</p>
<p>Accepts <code>slice:M/N</code> where this run executes slice <code>M</code> of <code>N</code> total slices (1-indexed). Tests are sorted by qualified name and then distributed by cycling through slices: test 1 to slice 1, test 2 to slice 2, ..., test N+1 to slice 1, and so on. Running every <code>slice:1/N</code> through <code>slice:N/N</code> together covers every collected test exactly once.</p>
<p>Useful for splitting a test run across CI jobs. Slice membership shifts when tests are added or removed, so it gives less stable per-test placement than a hash-based scheme.</p>
</dd><dt id="karva-test--profile"><a href="#karva-test--profile"><code>--profile</code></a>, <code>-P</code> <i>name</i></dt><dd><p>Configuration profile to use.</p>
<p>Profiles are defined as <code>&#91;profile.&lt;name&gt;&#93;</code> sections in <code>karva.toml</code> (or <code>&#91;tool.karva.profile.&lt;name&gt;&#93;</code> in <code>pyproject.toml</code>) and may override any of the <code>&#91;src&#93;</code>, <code>&#91;terminal&#93;</code>, and <code>&#91;test&#93;</code> settings. The selected profile is layered on top of any <code>&#91;profile.default&#93;</code> overrides, which themselves layer on top of the top-level options.</p>
<p>Defaults to <code>default</code>.</p>
<p>May also be set with the <code>KARVA_PROFILE</code> environment variable.</p></dd><dt id="karva-test--retry"><a href="#karva-test--retry"><code>--retry</code></a> <i>retry</i></dt><dd><p>When set, the test will retry failed tests up to this number of times</p>
</dd><dt id="karva-test--run-ignored"><a href="#karva-test--run-ignored"><code>--run-ignored</code></a> <i>run-ignored</i></dt><dd><p>Run ignored tests</p>
<p>Possible values:</p>
<ul>
<li><code>only</code>:  Run only ignored tests</li>
<li><code>all</code>:  Run both ignored and non-ignored tests</li>
</ul></dd><dt id="karva-test--show-output"><a href="#karva-test--show-output"><code>--show-output</code></a>, <code>-s</code></dt><dd><p>Show Python stdout during test execution</p>
</dd><dt id="karva-test--show-progress"><a href="#karva-test--show-progress"><code>--show-progress</code></a> <i>mode</i></dt><dd><p>Live progress display while tests run &#91;default: none&#93;</p>
<p><code>none</code> leaves output untouched. <code>counter</code> prints a refreshing <code>N/M tests</code> line on stderr. <code>bar</code> renders a visual progress bar. Stderr is used so the display does not interfere with per-test result lines on stdout.</p>
<p>May also be set with the <code>KARVA_SHOW_PROGRESS</code> environment variable.</p><p>Possible values:</p>
<ul>
<li><code>none</code>:  No live progress display (default)</li>
<li><code>counter</code>:  Print a one-line <code>N/M tests</code> counter, refreshed periodically</li>
<li><code>bar</code>:  Render a visual progress bar with completion stats</li>
</ul></dd><dt id="karva-test--slow-timeout"><a href="#karva-test--slow-timeout"><code>--slow-timeout</code></a> <i>seconds</i></dt><dd><p>Threshold in seconds after which a test is flagged as slow.</p>
<p>When a test takes longer than this duration, it is reported with a <code>SLOW</code> status line (gated on <code>--status-level=slow</code> or higher) and counted in the run summary. Pass a positive number such as <code>--slow-timeout=60</code> or <code>--slow-timeout=0.5</code>.</p>
</dd><dt id="karva-test--snapshot-update"><a href="#karva-test--snapshot-update"><code>--snapshot-update</code></a></dt><dd><p>Update snapshots directly instead of creating pending <code>.snap.new</code> files.</p>
<p>When set, <code>karva.assert_snapshot()</code> will write directly to <code>.snap</code> files, accepting any changes automatically.</p>
</dd><dt id="karva-test--status-level"><a href="#karva-test--status-level"><code>--status-level</code></a> <i>level</i></dt><dd><p>Test result statuses to display during the run &#91;default: pass&#93;</p>
<p>May also be set with the <code>KARVA_STATUS_LEVEL</code> environment variable.</p><p>Possible values:</p>
<ul>
<li><code>none</code>:  Don't display any test result lines (or the &quot;Starting&quot; header)</li>
<li><code>fail</code>:  Only display failed test results</li>
<li><code>retry</code>:  Display failed test results plus a <code>TRY N FAIL</code> line for each failed attempt that was retried</li>
<li><code>slow</code>:  Display failed, retried, and slow test results. Karva does not yet have a slow-test threshold, so this currently behaves like <code>retry</code></li>
<li><code>pass</code>:  Display failed, retried, slow, and passing test results (default)</li>
<li><code>skip</code>:  Additionally display skipped test results</li>
<li><code>all</code>:  Display all test result statuses</li>
</ul></dd><dt id="karva-test--test-prefix"><a href="#karva-test--test-prefix"><code>--test-prefix</code></a> <i>test-prefix</i></dt><dd><p>The prefix of the test functions</p>
</dd><dt id="karva-test--timeout"><a href="#karva-test--timeout"><code>--timeout</code></a> <i>seconds</i></dt><dd><p>Hard per-test timeout, in seconds.</p>
<p>Tests that run longer than this duration are killed and reported as failures. A test-level &#91;<code>@karva.tags.timeout</code>&#93; decorator overrides the default for that specific test.</p>
<p>Accepts fractional seconds such as <code>--timeout=120</code> or <code>--timeout=0.5</code>.</p>
</dd><dt id="karva-test--try-import-fixtures"><a href="#karva-test--try-import-fixtures"><code>--try-import-fixtures</code></a></dt><dd><p>When set, we will try to import functions in each test file as well as parsing the ast to find them.</p>
<p>This is often slower, so it is not recommended for most projects.</p>
</dd><dt id="karva-test--verbose"><a href="#karva-test--verbose"><code>--verbose</code></a>, <code>-v</code></dt><dd><p>Use verbose output (or <code>-vv</code> and <code>-vvv</code> for more verbose output)</p>
</dd><dt id="karva-test--watch"><a href="#karva-test--watch"><code>--watch</code></a></dt><dd><p>Re-run tests when Python source files change</p>
</dd></dl>

## karva snapshot

Manage snapshots created by `karva.assert_snapshot()`

<h3 class="cli-reference">Usage</h3>

```
karva snapshot <COMMAND>
```

<h3 class="cli-reference">Commands</h3>

<dl class="cli-reference"><dt><a href="#karva-snapshot-accept"><code>karva snapshot accept</code></a></dt><dd><p>Accept all (or filtered) pending snapshots</p></dd>
<dt><a href="#karva-snapshot-reject"><code>karva snapshot reject</code></a></dt><dd><p>Reject all (or filtered) pending snapshots</p></dd>
<dt><a href="#karva-snapshot-pending"><code>karva snapshot pending</code></a></dt><dd><p>List pending snapshots</p></dd>
<dt><a href="#karva-snapshot-review"><code>karva snapshot review</code></a></dt><dd><p>Interactively review pending snapshots</p></dd>
<dt><a href="#karva-snapshot-prune"><code>karva snapshot prune</code></a></dt><dd><p>Remove snapshot files whose source test no longer exists</p></dd>
<dt><a href="#karva-snapshot-delete"><code>karva snapshot delete</code></a></dt><dd><p>Delete all (or filtered) snapshot files (.snap and .snap.new)</p></dd>
<dt><a href="#karva-snapshot-help"><code>karva snapshot help</code></a></dt><dd><p>Print this message or the help of the given subcommand(s)</p></dd>
</dl>

### karva snapshot accept

Accept all (or filtered) pending snapshots

<h3 class="cli-reference">Usage</h3>

```
karva snapshot accept [PATH]...
```

<h3 class="cli-reference">Arguments</h3>

<dl class="cli-reference"><dt id="karva-snapshot-accept--paths"><a href="#karva-snapshot-accept--paths"><code>PATHS</code></a></dt><dd><p>Optional paths to filter snapshots by directory or file</p>
</dd></dl>

<h3 class="cli-reference">Options</h3>

<dl class="cli-reference"><dt id="karva-snapshot-accept--help"><a href="#karva-snapshot-accept--help"><code>--help</code></a>, <code>-h</code></dt><dd><p>Print help</p>
</dd></dl>

### karva snapshot reject

Reject all (or filtered) pending snapshots

<h3 class="cli-reference">Usage</h3>

```
karva snapshot reject [PATH]...
```

<h3 class="cli-reference">Arguments</h3>

<dl class="cli-reference"><dt id="karva-snapshot-reject--paths"><a href="#karva-snapshot-reject--paths"><code>PATHS</code></a></dt><dd><p>Optional paths to filter snapshots by directory or file</p>
</dd></dl>

<h3 class="cli-reference">Options</h3>

<dl class="cli-reference"><dt id="karva-snapshot-reject--help"><a href="#karva-snapshot-reject--help"><code>--help</code></a>, <code>-h</code></dt><dd><p>Print help</p>
</dd></dl>

### karva snapshot pending

List pending snapshots

<h3 class="cli-reference">Usage</h3>

```
karva snapshot pending [PATH]...
```

<h3 class="cli-reference">Arguments</h3>

<dl class="cli-reference"><dt id="karva-snapshot-pending--paths"><a href="#karva-snapshot-pending--paths"><code>PATHS</code></a></dt><dd><p>Optional paths to filter snapshots by directory or file</p>
</dd></dl>

<h3 class="cli-reference">Options</h3>

<dl class="cli-reference"><dt id="karva-snapshot-pending--help"><a href="#karva-snapshot-pending--help"><code>--help</code></a>, <code>-h</code></dt><dd><p>Print help</p>
</dd></dl>

### karva snapshot review

Interactively review pending snapshots

<h3 class="cli-reference">Usage</h3>

```
karva snapshot review [PATH]...
```

<h3 class="cli-reference">Arguments</h3>

<dl class="cli-reference"><dt id="karva-snapshot-review--paths"><a href="#karva-snapshot-review--paths"><code>PATHS</code></a></dt><dd><p>Optional paths to filter snapshots by directory or file</p>
</dd></dl>

<h3 class="cli-reference">Options</h3>

<dl class="cli-reference"><dt id="karva-snapshot-review--help"><a href="#karva-snapshot-review--help"><code>--help</code></a>, <code>-h</code></dt><dd><p>Print help</p>
</dd></dl>

### karva snapshot prune

Remove snapshot files whose source test no longer exists

<h3 class="cli-reference">Usage</h3>

```
karva snapshot prune [OPTIONS] [PATH]...
```

<h3 class="cli-reference">Arguments</h3>

<dl class="cli-reference"><dt id="karva-snapshot-prune--paths"><a href="#karva-snapshot-prune--paths"><code>PATHS</code></a></dt><dd><p>Optional paths to filter snapshots by directory or file</p>
</dd></dl>

<h3 class="cli-reference">Options</h3>

<dl class="cli-reference"><dt id="karva-snapshot-prune--dry-run"><a href="#karva-snapshot-prune--dry-run"><code>--dry-run</code></a></dt><dd><p>Show which snapshots would be removed without deleting them</p>
</dd><dt id="karva-snapshot-prune--help"><a href="#karva-snapshot-prune--help"><code>--help</code></a>, <code>-h</code></dt><dd><p>Print help</p>
</dd></dl>

### karva snapshot delete

Delete all (or filtered) snapshot files (.snap and .snap.new)

<h3 class="cli-reference">Usage</h3>

```
karva snapshot delete [OPTIONS] [PATH]...
```

<h3 class="cli-reference">Arguments</h3>

<dl class="cli-reference"><dt id="karva-snapshot-delete--paths"><a href="#karva-snapshot-delete--paths"><code>PATHS</code></a></dt><dd><p>Optional paths to filter which snapshot files are deleted</p>
</dd></dl>

<h3 class="cli-reference">Options</h3>

<dl class="cli-reference"><dt id="karva-snapshot-delete--dry-run"><a href="#karva-snapshot-delete--dry-run"><code>--dry-run</code></a></dt><dd><p>Show which snapshot files would be deleted without removing them</p>
</dd><dt id="karva-snapshot-delete--help"><a href="#karva-snapshot-delete--help"><code>--help</code></a>, <code>-h</code></dt><dd><p>Print help</p>
</dd></dl>

### karva snapshot help

Print this message or the help of the given subcommand(s)

<h3 class="cli-reference">Usage</h3>

```
karva snapshot help [COMMAND]
```

## karva cache

Manage the karva cache

<h3 class="cli-reference">Usage</h3>

```
karva cache <COMMAND>
```

<h3 class="cli-reference">Commands</h3>

<dl class="cli-reference"><dt><a href="#karva-cache-prune"><code>karva cache prune</code></a></dt><dd><p>Remove all but the most recent test run from the cache</p></dd>
<dt><a href="#karva-cache-clean"><code>karva cache clean</code></a></dt><dd><p>Remove the entire cache directory</p></dd>
<dt><a href="#karva-cache-help"><code>karva cache help</code></a></dt><dd><p>Print this message or the help of the given subcommand(s)</p></dd>
</dl>

### karva cache prune

Remove all but the most recent test run from the cache

<h3 class="cli-reference">Usage</h3>

```
karva cache prune
```

<h3 class="cli-reference">Options</h3>

<dl class="cli-reference"><dt id="karva-cache-prune--help"><a href="#karva-cache-prune--help"><code>--help</code></a>, <code>-h</code></dt><dd><p>Print help</p>
</dd></dl>

### karva cache clean

Remove the entire cache directory

<h3 class="cli-reference">Usage</h3>

```
karva cache clean
```

<h3 class="cli-reference">Options</h3>

<dl class="cli-reference"><dt id="karva-cache-clean--help"><a href="#karva-cache-clean--help"><code>--help</code></a>, <code>-h</code></dt><dd><p>Print help</p>
</dd></dl>

### karva cache help

Print this message or the help of the given subcommand(s)

<h3 class="cli-reference">Usage</h3>

```
karva cache help [COMMAND]
```

## karva version

Display Karva's version

<h3 class="cli-reference">Usage</h3>

```
karva version
```

<h3 class="cli-reference">Options</h3>

<dl class="cli-reference"><dt id="karva-version--help"><a href="#karva-version--help"><code>--help</code></a>, <code>-h</code></dt><dd><p>Print help</p>
</dd></dl>

## karva help

Print this message or the help of the given subcommand(s)

<h3 class="cli-reference">Usage</h3>

```
karva help [COMMAND]
```

