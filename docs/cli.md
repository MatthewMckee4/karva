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

<dl class="cli-reference"><dt id="karva-test--paths"><a href="#karva-test--paths"><code>PATHS</code></a></dt><dd><p>List of files, directories, or test functions to test [default: the project root]</p>
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
<p>May also be set with the <code>KARVA_CONFIG_FILE</code> environment variable.</p></dd><dt id="karva-test--dry-run"><a href="#karva-test--dry-run"><code>--dry-run</code></a></dt><dd><p>Print discovered tests without executing them</p>
</dd><dt id="karva-test--fail-fast"><a href="#karva-test--fail-fast"><code>--fail-fast</code></a></dt><dd><p>When set, the test will fail immediately if any test fails.</p>
<p>This only works when running tests in parallel.</p>
</dd><dt id="karva-test--help"><a href="#karva-test--help"><code>--help</code></a>, <code>-h</code></dt><dd><p>Print help (see a summary with '-h')</p>
</dd><dt id="karva-test--match"><a href="#karva-test--match"><code>--match</code></a>, <code>-m</code> <i>name-patterns</i></dt><dd><p>Filter tests by name using a regular expression.</p>
<p>Only tests whose fully qualified name matches the pattern will run. Uses partial matching (the pattern can match anywhere in the name). When specified multiple times, a test runs if it matches any of the patterns.</p>
<p>Examples: <code>-m auth</code>, <code>-m '^test::test_login'</code>, <code>-m 'slow|fast'</code>.</p>
</dd><dt id="karva-test--no-cache"><a href="#karva-test--no-cache"><code>--no-cache</code></a></dt><dd><p>Disable reading the karva cache for test duration history</p>
</dd><dt id="karva-test--no-ignore"><a href="#karva-test--no-ignore"><code>--no-ignore</code></a></dt><dd><p>When set, .gitignore files will not be respected</p>
</dd><dt id="karva-test--no-parallel"><a href="#karva-test--no-parallel"><code>--no-parallel</code></a></dt><dd><p>Disable parallel execution (equivalent to <code>--num-workers 1</code>)</p>
</dd><dt id="karva-test--no-progress"><a href="#karva-test--no-progress"><code>--no-progress</code></a></dt><dd><p>When set, we will not show individual test case results during execution</p>
</dd><dt id="karva-test--num-workers"><a href="#karva-test--num-workers"><code>--num-workers</code></a>, <code>-n</code> <i>num-workers</i></dt><dd><p>Number of parallel workers (default: number of CPU cores)</p>
</dd><dt id="karva-test--output-format"><a href="#karva-test--output-format"><code>--output-format</code></a> <i>output-format</i></dt><dd><p>The format to use for printing diagnostic messages</p>
<p>Possible values:</p>
<ul>
<li><code>full</code>:  Print diagnostics verbosely, with context and helpful hints (default)</li>
<li><code>concise</code>:  Print diagnostics concisely, one per line</li>
</ul></dd><dt id="karva-test--quiet"><a href="#karva-test--quiet"><code>--quiet</code></a>, <code>-q</code></dt><dd><p>Use quiet output (or <code>-qq</code> for silent output)</p>
</dd><dt id="karva-test--retry"><a href="#karva-test--retry"><code>--retry</code></a> <i>retry</i></dt><dd><p>When set, the test will retry failed tests up to this number of times</p>
</dd><dt id="karva-test--snapshot-update"><a href="#karva-test--snapshot-update"><code>--snapshot-update</code></a></dt><dd><p>Update snapshots directly instead of creating pending <code>.snap.new</code> files.</p>
<p>When set, <code>karva.assert_snapshot()</code> will write directly to <code>.snap</code> files, accepting any changes automatically.</p>
</dd><dt id="karva-test--tag"><a href="#karva-test--tag"><code>--tag</code></a>, <code>-t</code> <i>tag-expressions</i></dt><dd><p>Filter tests by tag expression. Only tests with matching custom tags will run.</p>
<p>Expressions support <code>and</code>, <code>or</code>, <code>not</code>, and parentheses for grouping. When specified multiple times, a test runs if it matches any of the expressions.</p>
<p>Examples: <code>-t slow</code>, <code>-t 'not slow'</code>, <code>-t 'slow and integration'</code>, <code>-t 'slow or integration'</code>, <code>-t '(slow or fast) and not flaky'</code>.</p>
</dd><dt id="karva-test--test-prefix"><a href="#karva-test--test-prefix"><code>--test-prefix</code></a> <i>test-prefix</i></dt><dd><p>The prefix of the test functions</p>
</dd><dt id="karva-test--try-import-fixtures"><a href="#karva-test--try-import-fixtures"><code>--try-import-fixtures</code></a></dt><dd><p>When set, we will try to import functions in each test file as well as parsing the ast to find them.</p>
<p>This is often slower, so it is not recommended for most projects.</p>
</dd><dt id="karva-test--verbose"><a href="#karva-test--verbose"><code>--verbose</code></a>, <code>-v</code></dt><dd><p>Use verbose output (or <code>-vv</code> and <code>-vvv</code> for more verbose output)</p>
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

