# Changelog

## 0.0.1-alpha.8

### Bug Fixes

- Preserve snapshot context for timed tests ([#1021](https://github.com/MatthewMckee4/karva/pull/1021))
- Normalize command snapshot line endings ([#1017](https://github.com/MatthewMckee4/karva/pull/1017))
- Avoid files for inline snapshot mismatches ([#1015](https://github.com/MatthewMckee4/karva/pull/1015))
- Capture subprocess output with capfd ([#1002](https://github.com/MatthewMckee4/karva/pull/1002))

### Documentation

- Improve contributor guides ([#1019](https://github.com/MatthewMckee4/karva/pull/1019))
- Document ANSI command snapshot filtering ([#1016](https://github.com/MatthewMckee4/karva/pull/1016))

### Contributors

- [@MatthewMckee4](https://github.com/MatthewMckee4)

## 0.0.1-alpha.7

### Bug Fixes

- Report fixture dependency cycles ([#995](https://github.com/MatthewMckee4/karva/pull/995))
- Stabilize snapshot file names ([#992](https://github.com/MatthewMckee4/karva/pull/992))
- Reject generator test functions ([#972](https://github.com/MatthewMckee4/karva/pull/972))
- Fail tests that return values ([#971](https://github.com/MatthewMckee4/karva/pull/971))
- Ignore commented defs when pruning snapshots ([#970](https://github.com/MatthewMckee4/karva/pull/970))
- Report scheduled test count in start banner ([#943](https://github.com/MatthewMckee4/karva/pull/943))
- Reject zero test workers ([#938](https://github.com/MatthewMckee4/karva/pull/938))
- Allow explicit false for bool test flags ([#930](https://github.com/MatthewMckee4/karva/pull/930))
- Treat snapshot filter replacements literally ([#929](https://github.com/MatthewMckee4/karva/pull/929))
- Ignore directory worker binary candidates ([#927](https://github.com/MatthewMckee4/karva/pull/927))
- Normalize interrupted parametrized failures ([#925](https://github.com/MatthewMckee4/karva/pull/925))
- Avoid retrying skipped and expected-fail tests ([#924](https://github.com/MatthewMckee4/karva/pull/924))
- Fix HTML coverage branch total ([#919](https://github.com/MatthewMckee4/karva/pull/919))
- Fix snapshot source path filters ([#918](https://github.com/MatthewMckee4/karva/pull/918))
- Forward configured retries to workers ([#914](https://github.com/MatthewMckee4/karva/pull/914))
- Fix show-config serialization for overrides ([#786](https://github.com/MatthewMckee4/karva/pull/786))

### CLI

- Add structured result reports ([#892](https://github.com/MatthewMckee4/karva/pull/892))
- Add graceful worker termination ([#890](https://github.com/MatthewMckee4/karva/pull/890))
- Add hash test partitioning ([#889](https://github.com/MatthewMckee4/karva/pull/889))
- Add branch coverage ([#885](https://github.com/MatthewMckee4/karva/pull/885))
- Add coverage include and omit filters ([#881](https://github.com/MatthewMckee4/karva/pull/881))

### Configuration

- Enable Seal pull request creation ([#996](https://github.com/MatthewMckee4/karva/pull/996))
- Cover configured fixture imports ([#916](https://github.com/MatthewMckee4/karva/pull/916))
- Cover configured slow timeout ([#915](https://github.com/MatthewMckee4/karva/pull/915))
- Add JUnit XML reports ([#887](https://github.com/MatthewMckee4/karva/pull/887))

### Coverage

- Add coverage.py data file ([#884](https://github.com/MatthewMckee4/karva/pull/884))
- Add coverage test contexts ([#883](https://github.com/MatthewMckee4/karva/pull/883))
- Cache coverage paths per code object ([#882](https://github.com/MatthewMckee4/karva/pull/882))

### Diagnostics

- Preserve argument order in test names ([#994](https://github.com/MatthewMckee4/karva/pull/994))
- Hide framework fixture values from test names ([#991](https://github.com/MatthewMckee4/karva/pull/991))
- Prefer pytest fixture validation errors ([#903](https://github.com/MatthewMckee4/karva/pull/903))
- Capture output per test ([#886](https://github.com/MatthewMckee4/karva/pull/886))
- Reduce status output progress tracking overhead ([#804](https://github.com/MatthewMckee4/karva/pull/804))

### Documentation

- Document project non-goals ([#878](https://github.com/MatthewMckee4/karva/pull/878))
- Add README support and license sections ([#839](https://github.com/MatthewMckee4/karva/pull/839))

### Contributors

- [@MatthewMckee4](https://github.com/MatthewMckee4)

## 0.0.1-alpha.6

### Bug Fixes

- Make CoverageTracer thread-safe to fix --cov panic on threaded code ([#761](https://github.com/MatthewMckee4/karva/pull/761))

### CLI

- Add per-test hard timeout configuration ([#768](https://github.com/MatthewMckee4/karva/pull/768))
- Add --partition slice:M/N for round-robin test slicing ([#767](https://github.com/MatthewMckee4/karva/pull/767))
- Add `--no-cov` to disable coverage for a single run ([#733](https://github.com/MatthewMckee4/karva/pull/733))
- Add `--cov-report=term-missing` to show uncovered lines per file ([#716](https://github.com/MatthewMckee4/karva/pull/716))

### Configuration

- Add required-version field to configuration ([#771](https://github.com/MatthewMckee4/karva/pull/771))
- Expose coverage configuration via `[coverage]` ([#730](https://github.com/MatthewMckee4/karva/pull/730))

### Coverage

- Support `# pragma: no cover` exclusion comments in coverage ([#736](https://github.com/MatthewMckee4/karva/pull/736))
- Include never-imported source files at 0% in coverage report ([#735](https://github.com/MatthewMckee4/karva/pull/735))
- Add `--cov-fail-under` to gate the run on a coverage threshold ([#734](https://github.com/MatthewMckee4/karva/pull/734))

### Diagnostics

- Print nextest-style cancellation banner on Ctrl+C ([#764](https://github.com/MatthewMckee4/karva/pull/764))
- Make context window size 0 for diagnostics ([#758](https://github.com/MatthewMckee4/karva/pull/758))
- Add `--slow-timeout` and SLOW reporting ([#731](https://github.com/MatthewMckee4/karva/pull/731))

### Documentation

- Fix zensical build warnings and bump zensical to 0.0.38 ([#746](https://github.com/MatthewMckee4/karva/pull/746))
- Add docs pages for coverage, parallelism, retries, watch, slow tests, fail-fast, cache, timeout tag ([#738](https://github.com/MatthewMckee4/karva/pull/738))
- Expose `KARVA`, `KARVA_WORKER_ID`, `KARVA_RUN_ID`, `KARVA_WORKSPACE_ROOT`, and `KARVA_TEST_NAME` to tests ([#728](https://github.com/MatthewMckee4/karva/pull/728))

### Test Running

- Expose KARVA_PROFILE, KARVA_TEST_THREADS, and KARVA_VERSION to tests ([#741](https://github.com/MatthewMckee4/karva/pull/741))
- Expose `KARVA_ATTEMPT` and `KARVA_TOTAL_ATTEMPTS` to tests ([#715](https://github.com/MatthewMckee4/karva/pull/715))

### Contributors

- [@MatthewMckee4](https://github.com/MatthewMckee4)
- [@tyanochkuby](https://github.com/tyanochkuby)

## 0.0.1-alpha.5

### Bug Fixes

- Fix snapshot diff trailing-newline and show relative paths ([#709](https://github.com/MatthewMckee4/karva/pull/709))
- Fix incorrect `karva --version` ([#682](https://github.com/MatthewMckee4/karva/pull/682))
- fix: apply filter expressions in --dry-run and make -qq silent ([#671](https://github.com/MatthewMckee4/karva/pull/671))
- Add regression tests for autouse fixtures from subdirectory conftests ([#644](https://github.com/MatthewMckee4/karva/pull/644))
- Add regression test for monkeypatch.setattr(module, attr, None) ([#642](https://github.com/MatthewMckee4/karva/pull/642))
- Have capsys/capfd save and restore logging.disable level ([#641](https://github.com/MatthewMckee4/karva/pull/641))
- Add handler attribute to caplog fixture ([#640](https://github.com/MatthewMckee4/karva/pull/640))
- Discover pytest fixtures imported into conftest.py ([#639](https://github.com/MatthewMckee4/karva/pull/639))
- Fix capsysbinary to accept bytes writes to sys.stdout/sys.stderr ([#637](https://github.com/MatthewMckee4/karva/pull/637))
- Remove logging.disable(CRITICAL) from redirect_python_output ([#636](https://github.com/MatthewMckee4/karva/pull/636))
- Fix get_auto_use_fixtures collecting only first autouse fixture ([#635](https://github.com/MatthewMckee4/karva/pull/635))
- Fix monkeypatch.context() so __exit__ undoes patches from the yielded instance ([#631](https://github.com/MatthewMckee4/karva/pull/631))
- Add record_tuples property to caplog fixture ([#629](https://github.com/MatthewMckee4/karva/pull/629))
- Fix monkeypatch.setattr(obj, attr, None) and caplog record.message ([#626](https://github.com/MatthewMckee4/karva/pull/626))
- Fix function-scoped built-in fixtures not isolated across parametrize variants ([#616](https://github.com/MatthewMckee4/karva/pull/616))
- Make collection errors non-fatal diagnostics ([#613](https://github.com/MatthewMckee4/karva/pull/613))
- Fix monkeypatch.setattr() dotted import string form ([#611](https://github.com/MatthewMckee4/karva/pull/611))
- Stop project discovery at .git boundary ([#610](https://github.com/MatthewMckee4/karva/pull/610))
- Fix inline snapshot closing `"""` indentation ([#496](https://github.com/MatthewMckee4/karva/pull/496))
- Fix inline snapshot corruption on multiline accept + partial accept workflow tests ([#494](https://github.com/MatthewMckee4/karva/pull/494))

### CLI

- Emit per-attempt retry lines and summary counter ([#701](https://github.com/MatthewMckee4/karva/pull/701))
- Add nextest-style configuration profiles ([#700](https://github.com/MatthewMckee4/karva/pull/700))
- add --max-fail=N to stop after N failures ([#666](https://github.com/MatthewMckee4/karva/pull/666))
- Add filterset DSL for test selection ([#663](https://github.com/MatthewMckee4/karva/pull/663))
- Adopt nextest-style output format ([#599](https://github.com/MatthewMckee4/karva/pull/599))
- Support `--fail-fast` across workers via file-based signal ([#499](https://github.com/MatthewMckee4/karva/pull/499))
- Add `karva cache prune` and `karva cache clean` commands ([#498](https://github.com/MatthewMckee4/karva/pull/498))

### Documentation

- docs: document caplog, capsys, capfd, recwarn, tmp_path_factory built-in fixtures ([#664](https://github.com/MatthewMckee4/karva/pull/664))
- Add complete snapshot documentation ([#495](https://github.com/MatthewMckee4/karva/pull/495))

### Extensions

- Add `@karva.tags.timeout(seconds)` decorator ([#710](https://github.com/MatthewMckee4/karva/pull/710))
- Make tmp_path_factory and tmpdir_factory session-scoped ([#638](https://github.com/MatthewMckee4/karva/pull/638))
- Add capsysbinary and capfdbinary built-in fixtures ([#630](https://github.com/MatthewMckee4/karva/pull/630))
- Add recwarn built-in fixture ([#612](https://github.com/MatthewMckee4/karva/pull/612))
- Add capsys built-in fixture ([#608](https://github.com/MatthewMckee4/karva/pull/608))
- Add caplog built-in fixture ([#607](https://github.com/MatthewMckee4/karva/pull/607))

### Snapshot Testing

- Use more snapshot tests and add integration tests ([#497](https://github.com/MatthewMckee4/karva/pull/497))

### Contributors

- [@MatthewMckee4](https://github.com/MatthewMckee4)
- [@OmChillure](https://github.com/OmChillure)

## 0.0.1-alpha.4

### CLI

- Add `--watch` flag to `karva test` ([#486](https://github.com/MatthewMckee4/karva/pull/486))
- Add `--dry-run` flag to `karva test` ([#479](https://github.com/MatthewMckee4/karva/pull/479))

### Extensions

- Show span annotations for each fixture in dependency chain ([#488](https://github.com/MatthewMckee4/karva/pull/488))
- Show fixture dependency chain in error messages ([#487](https://github.com/MatthewMckee4/karva/pull/487))
- Fully support async tests and fixtures ([#485](https://github.com/MatthewMckee4/karva/pull/485))

### Snapshot Testing

- Add assert_cmd_snapshot function and Command class ([#461](https://github.com/MatthewMckee4/karva/pull/461))
- Add `assert_json_snapshot` function ([#458](https://github.com/MatthewMckee4/karva/pull/458))
- Add `name=` parameter to `assert_snapshot` for named snapshots ([#457](https://github.com/MatthewMckee4/karva/pull/457))
- Add `karva snapshot delete` command and fix snapshot path filtering ([#455](https://github.com/MatthewMckee4/karva/pull/455))
- Add snapshot_settings context manager with filter support ([#454](https://github.com/MatthewMckee4/karva/pull/454))
- Add `karva snapshot prune` command ([#453](https://github.com/MatthewMckee4/karva/pull/453))
- Add inline snapshots (insta-style) ([#450](https://github.com/MatthewMckee4/karva/pull/450))
- Add snapshot testing ([#444](https://github.com/MatthewMckee4/karva/pull/444))

### Contributors

- [@MatthewMckee4](https://github.com/MatthewMckee4)

## 0.0.1-alpha.3

### Extensions

- Add `-t` / `--tag` flag for filtering tests by custom tag expressions ([#422](https://github.com/MatthewMckee4/karva/pull/422))

### Test Running

- Add `karva.raises` context manager for asserting exceptions ([#430](https://github.com/MatthewMckee4/karva/pull/430))
- Add `-m` / `--match` flag for regex-based test name filtering ([#428](https://github.com/MatthewMckee4/karva/pull/428))
- Replace body_length heuristic with random ordering ([#425](https://github.com/MatthewMckee4/karva/pull/425))

### Contributors

- [@MatthewMckee4](https://github.com/MatthewMckee4)

## 0.0.1-alpha.2

### Bug Fixes

- Fix ctrl-c ([#357](https://github.com/MatthewMckee4/karva/pull/357))
- Fix run hash timestamp ([#356](https://github.com/MatthewMckee4/karva/pull/356))
- Fix `pytest.parametrize` with kwargs ([#342](https://github.com/MatthewMckee4/karva/pull/342))

### CLI

- Add --no-cache flag to disable reading cache ([#400](https://github.com/MatthewMckee4/karva/pull/400))

### Documentation

- Document that --no-parallel is equivalent to --num-workers 1 ([#399](https://github.com/MatthewMckee4/karva/pull/399))
- Update documentation URLs to matthewmckee4.github.io ([#398](https://github.com/MatthewMckee4/karva/pull/398))
- Add disclaimer to docs that we won't support request ([#387](https://github.com/MatthewMckee4/karva/pull/387))
- Remove README note ([#340](https://github.com/MatthewMckee4/karva/pull/340))

### Extensions

- Remove `request` and fixture params ([#384](https://github.com/MatthewMckee4/karva/pull/384))
- Request node and custom tags ([#352](https://github.com/MatthewMckee4/karva/pull/352))
- Try import fixtures ([#351](https://github.com/MatthewMckee4/karva/pull/351))

### Test Running

- Support retrying tests ([#354](https://github.com/MatthewMckee4/karva/pull/354))

### Contributors

- [@MatthewMckee4](https://github.com/MatthewMckee4)

## 0.0.1-alpha.1

Since karva has been re-released, this is the first proper pre-release.

This means that not all of the changes will be documented in this changelog.
See the documentation for more information.

### Bug Fixes

- Follow symlinks in directory walker ([#307](https://github.com/MatthewMckee4/karva/pull/307))
- Dont import all files in discovery ([#269](https://github.com/MatthewMckee4/karva/pull/269))
- Support dependent fixtures ([#70](https://github.com/MatthewMckee4/karva/pull/70))
- Add initial pytest fixture parsing ([#69](https://github.com/MatthewMckee4/karva/pull/69))
- Fix karva fail when no path provided ([#23](https://github.com/MatthewMckee4/karva/pull/23))

### Configuration

- Support configuration files ([#317](https://github.com/MatthewMckee4/karva/pull/317))

### Extensions

- Support `karva.param` in fixtures ([#289](https://github.com/MatthewMckee4/karva/pull/289))
- Support `karva.param` in parametrized tests ([#288](https://github.com/MatthewMckee4/karva/pull/288))
- Support `pytest.param` in `tags.parametrize` ([#279](https://github.com/MatthewMckee4/karva/pull/279))
- Support mocked environment fixture ([#277](https://github.com/MatthewMckee4/karva/pull/277))
- Support dynamically imported fixtures ([#256](https://github.com/MatthewMckee4/karva/pull/256))
- Support pytest param in fixtures ([#250](https://github.com/MatthewMckee4/karva/pull/250))
- Support expect fail ([#243](https://github.com/MatthewMckee4/karva/pull/243))
- Add diagnostics for fixtures having missing fixtures ([#232](https://github.com/MatthewMckee4/karva/pull/232))
- Show fixture diagnostics ([#231](https://github.com/MatthewMckee4/karva/pull/231))
- Support skip if ([#228](https://github.com/MatthewMckee4/karva/pull/228))
- Support skip in function ([#227](https://github.com/MatthewMckee4/karva/pull/227))
- Support parametrize args in a single string ([#187](https://github.com/MatthewMckee4/karva/pull/187))
- Allow fixture override ([#129](https://github.com/MatthewMckee4/karva/pull/129))
- Add support for dynamic fixture scopes ([#124](https://github.com/MatthewMckee4/karva/pull/124))

### Reporting

- Use ruff diagnostics ([#275](https://github.com/MatthewMckee4/karva/pull/275))

### Contributors

- [@MatthewMckee4](https://github.com/MatthewMckee4)
- [@bschoenmaeckers](https://github.com/bschoenmaeckers)
- [@my1e5](https://github.com/my1e5)
