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
<p>While ty configuration can be included in a <code>pyproject.toml</code> file, it is not allowed in this context.</p>
<p>May also be set with the <code>KARVA_CONFIG_FILE</code> environment variable.</p></dd><dt id="karva-test--fail-fast"><a href="#karva-test--fail-fast"><code>--fail-fast</code></a></dt><dd><p>When set, the test will fail immediately if any test fails.</p>
<p>This only works when running tests in parallel.</p>
</dd><dt id="karva-test--help"><a href="#karva-test--help"><code>--help</code></a>, <code>-h</code></dt><dd><p>Print help (see a summary with '-h')</p>
</dd><dt id="karva-test--no-ignore"><a href="#karva-test--no-ignore"><code>--no-ignore</code></a></dt><dd><p>When set, .gitignore files will not be respected</p>
</dd><dt id="karva-test--no-parallel"><a href="#karva-test--no-parallel"><code>--no-parallel</code></a></dt><dd><p>Disable parallel execution</p>
</dd><dt id="karva-test--no-progress"><a href="#karva-test--no-progress"><code>--no-progress</code></a></dt><dd><p>When set, we will not show individual test case results during execution</p>
</dd><dt id="karva-test--num-workers"><a href="#karva-test--num-workers"><code>--num-workers</code></a>, <code>-n</code> <i>num-workers</i></dt><dd><p>Number of parallel workers (default: number of CPU cores)</p>
</dd><dt id="karva-test--output-format"><a href="#karva-test--output-format"><code>--output-format</code></a> <i>output-format</i></dt><dd><p>The format to use for printing diagnostic messages</p>
<p>Possible values:</p>
<ul>
<li><code>full</code>:  Print diagnostics verbosely, with context and helpful hints (default)</li>
<li><code>concise</code>:  Print diagnostics concisely, one per line</li>
</ul></dd><dt id="karva-test--quiet"><a href="#karva-test--quiet"><code>--quiet</code></a>, <code>-q</code></dt><dd><p>Use quiet output (or <code>-qq</code> for silent output)</p>
</dd><dt id="karva-test--retry"><a href="#karva-test--retry"><code>--retry</code></a> <i>retry</i></dt><dd><p>When set, the test will retry failed tests up to this number of times</p>
</dd><dt id="karva-test--test-prefix"><a href="#karva-test--test-prefix"><code>--test-prefix</code></a> <i>test-prefix</i></dt><dd><p>The prefix of the test functions</p>
</dd><dt id="karva-test--try-import-fixtures"><a href="#karva-test--try-import-fixtures"><code>--try-import-fixtures</code></a></dt><dd><p>When set, we will try to import functions in each test file as well as parsing the ast to find them.</p>
<p>This is often slower, so it is not recommended for most projects.</p>
</dd><dt id="karva-test--verbose"><a href="#karva-test--verbose"><code>--verbose</code></a>, <code>-v</code></dt><dd><p>Use verbose output (or <code>-vv</code> and <code>-vvv</code> for more verbose output)</p>
</dd></dl>

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

