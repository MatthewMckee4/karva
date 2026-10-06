---
title: Karva
description: A Python test framework, written in Rust.
---

<div class="home-hero">
  <div class="home-intro">
    <h1>Karva</h1>
    <p class="home-lede">A Python test framework, written in Rust.</p>
    <p>Run Python tests in parallel.</p>
    <p>In alpha. <a href="usage/non-goals/">Check compatibility</a> before migrating.</p>
    <p class="hero-actions">
      <a class="md-button md-button--primary" href="get-started/tutorial/">Run your first test</a>
      <a class="md-button" href="usage/">Read the guide</a>
    </p>
  </div>
  <div class="command-panel">
    <span>Get started</span>
    <pre><code><span>uv add --dev karva</span><span>uv run karva test</span></code></pre>
  </div>
</div>

<section class="evidence-section">
  <div>
    <h2>Benchmarks</h2>
    <p>Results depend on your suite and machine.</p>
  </div>
  <div class="benchmark-chart" role="group" aria-label="Benchmark runtime comparison">
    <div class="benchmark-row benchmark-row--karva">
      <strong>karva</strong><progress max="93" value="2.6" aria-label="Karva: 2.6 seconds">2.6 seconds</progress><span>2.60s</span>
    </div>
    <div class="benchmark-row benchmark-row--xdist">
      <strong>pytest-xdist</strong><progress max="93" value="60.5" aria-label="pytest-xdist: 60.5 seconds">60.5 seconds</progress><span>60.50s</span>
    </div>
    <div class="benchmark-row benchmark-row--pytest">
      <strong>pytest</strong><progress max="93" value="92.2" aria-label="pytest: 92.2 seconds">92.2 seconds</progress><span>92.20s</span>
    </div>
    <small>Workload: approximately 250,000 tests · Machine: 14 cores · <a href="https://github.com/MatthewMckee4/karva-benchmark-1">Benchmark project</a></small>
  </div>
</section>

<section class="built-in-section">
  <div class="section-heading">
    <h2>Built in</h2>
  </div>
  <div class="capability-grid">
    <a href="usage/running-tests/parallel/"><strong>Parallel execution</strong><span>Separate Python workers</span></a>
    <a href="usage/running-tests/filtering/"><strong>Filtering</strong><span>Select tests with expressions</span></a>
    <a href="usage/running-tests/watch/"><strong>Watch mode</strong><span>Rerun after source changes</span></a>
    <a href="usage/writing-tests/coverage/"><strong>Coverage</strong><span>Native line coverage</span></a>
    <a href="usage/writing-tests/snapshots/"><strong>Snapshots</strong><span>File and inline snapshots</span></a>
    <a href="usage/fixtures/fixtures/"><strong>Fixtures</strong><span>Share setup and teardown</span></a>
  </div>
</section>
