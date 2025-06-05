<!-- WARNING: This file is auto-generated (cargo dev generate-all). Edit the doc comments in 'crates/karva_cli/src/args.rs' if you want to change anything here. -->

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

<dl class="cli-reference"><dt id="karva-test--help"><a href="#karva-test--help"><code>--help</code></a>, <code>-h</code></dt><dd><p>Print help</p>
</dd><dt id="karva-test--test-prefix"><a href="#karva-test--test-prefix"><code>--test-prefix</code></a>, <code>-p</code> <i>test-prefix</i></dt><dd><p>The prefix of the test functions</p>
<p>[default: test]</p></dd><dt id="karva-test--verbose"><a href="#karva-test--verbose"><code>--verbose</code></a>, <code>-v</code></dt><dd><p>Use verbose output (or <code>-vv</code> and <code>-vvv</code> for more verbose output)</p>
</dd><dt id="karva-test--watch"><a href="#karva-test--watch"><code>--watch</code></a></dt><dd><p>Run in watch mode</p>
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

