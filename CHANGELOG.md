# Changelog

## 0.0.1-alpha.2

### Bug Fixes

- Fix ctrl-c ([#357](https://github.com/karva-dev/karva/pull/357))
- Fix run hash timestamp ([#356](https://github.com/karva-dev/karva/pull/356))
- Fix `pytest.parametrize` with kwargs ([#342](https://github.com/karva-dev/karva/pull/342))

### CLI

- Add --no-cache flag to disable reading cache ([#400](https://github.com/karva-dev/karva/pull/400))

### Documentation

- Document that --no-parallel is equivalent to --num-workers 1 ([#399](https://github.com/karva-dev/karva/pull/399))
- Update documentation URLs to karva-dev.github.io ([#398](https://github.com/karva-dev/karva/pull/398))
- Add disclaimer to docs that we won't support request ([#387](https://github.com/karva-dev/karva/pull/387))
- Remove README note ([#340](https://github.com/karva-dev/karva/pull/340))

### Extensions

- Remove `request` and fixture params ([#384](https://github.com/karva-dev/karva/pull/384))
- Request node and custom tags ([#352](https://github.com/karva-dev/karva/pull/352))
- Try import fixtures ([#351](https://github.com/karva-dev/karva/pull/351))

### Test Running

- Support retrying tests ([#354](https://github.com/karva-dev/karva/pull/354))

### Contributors

- [@MatthewMckee4](https://github.com/MatthewMckee4)

## 0.0.1-alpha.1

Since karva has been re-released, this is the first proper pre-release.

This means that not all of the changes will be documented in this changelog.
See the documentation for more information.

### Bug Fixes

- Follow symlinks in directory walker ([#307](https://github.com/karva-dev/karva/pull/307))
- Dont import all files in discovery ([#269](https://github.com/karva-dev/karva/pull/269))
- Support dependent fixtures ([#70](https://github.com/karva-dev/karva/pull/70))
- Add initial pytest fixture parsing ([#69](https://github.com/karva-dev/karva/pull/69))
- Fix karva fail when no path provided ([#23](https://github.com/karva-dev/karva/pull/23))

### Configuration

- Support configuration files ([#317](https://github.com/karva-dev/karva/pull/317))

### Extensions

- Support `karva.param` in fixtures ([#289](https://github.com/karva-dev/karva/pull/289))
- Support `karva.param` in parametrized tests ([#288](https://github.com/karva-dev/karva/pull/288))
- Support `pytest.param` in `tags.parametrize` ([#279](https://github.com/karva-dev/karva/pull/279))
- Support mocked environment fixture ([#277](https://github.com/karva-dev/karva/pull/277))
- Support dynamically imported fixtures ([#256](https://github.com/karva-dev/karva/pull/256))
- Support pytest param in fixtures ([#250](https://github.com/karva-dev/karva/pull/250))
- Support expect fail ([#243](https://github.com/karva-dev/karva/pull/243))
- Add diagnostics for fixtures having missing fixtures ([#232](https://github.com/karva-dev/karva/pull/232))
- Show fixture diagnostics ([#231](https://github.com/karva-dev/karva/pull/231))
- Support skip if ([#228](https://github.com/karva-dev/karva/pull/228))
- Support skip in function ([#227](https://github.com/karva-dev/karva/pull/227))
- Support parametrize args in a single string ([#187](https://github.com/karva-dev/karva/pull/187))
- Allow fixture override ([#129](https://github.com/karva-dev/karva/pull/129))
- Add support for dynamic fixture scopes ([#124](https://github.com/karva-dev/karva/pull/124))

### Reporting

- Use ruff diagnostics ([#275](https://github.com/karva-dev/karva/pull/275))

### Contributors

- [@MatthewMckee4](https://github.com/MatthewMckee4)
- [@bschoenmaeckers](https://github.com/bschoenmaeckers)
- [@my1e5](https://github.com/my1e5)
