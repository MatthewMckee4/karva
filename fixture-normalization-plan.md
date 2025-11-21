# Fixture Normalization Plan

## Overview

The normalization step occurs **after discovery** and **before running tests**. It transforms discovered test functions and fixtures into normalized variants that explicitly account for parametrization and fixture dependencies.

## Goals

1. Convert parametrized fixtures into multiple normalized fixtures, one for each parameter value
2. Propagate parametrization through the dependency chain (cartesian product)
3. Maintain test identity for reporting (same test, different params)
4. Eliminate runtime parametrization complexity by pre-computing all test/fixture variants

## Core Concepts

### Fixture Splitting

When a fixture has parameters, we "split" it into multiple normalized fixtures:

```
Original:
  @pytest.fixture(params=[1, 2, 3])
  def number():
      ...

Normalized:
  number[1] -> NormalizedFixture with param=1
  number[2] -> NormalizedFixture with param=2
  number[3] -> NormalizedFixture with param=3
```

Each normalized fixture gets:
- A unique name: `{fixture_name}[{stringified_param}]`
- A concrete parameter value (no runtime parametrization)
- Its own set of normalized dependencies

### Cartesian Product for Dependencies

When a fixture or test function depends on parametrized fixtures, we create a normalized variant for each combination:

```
Fixtures:
  @pytest.fixture(params=[1, 2])
  def x():
      ...

  @pytest.fixture(params=['a', 'b'])
  def y():
      ...

  def test_example(x, y):
      ...

Normalized:
  test_example[x=1,y='a']
  test_example[x=1,y='b']
  test_example[x=2,y='a']
  test_example[x=2,y='b']
```

## Normalization Algorithm

### 1. Normalize a Single Fixture

```rust
fn normalize_fixture(
    fixture: &Fixture,
    fixture_manager: &FixtureManager,
) -> Vec<NormalizedFixture> {
    // Get all required fixtures (dependencies)
    let dependencies = fixture.required_fixtures();

    // Recursively normalize each dependency
    let mut normalized_deps: Vec<Vec<NormalizedFixture>> = vec![];
    for dep_name in dependencies {
        let dep_fixture = fixture_manager.get(dep_name);
        let normalized = normalize_fixture(dep_fixture, fixture_manager);
        normalized_deps.push(normalized);
    }

    // Get fixture parameters
    let params = fixture.params();

    if params.is_empty() && all_deps_have_single_variant(normalized_deps) {
        // No parametrization needed
        return vec![create_normalized_fixture(fixture, params=None, deps)];
    }

    // Create cartesian product of dependencies and parameters
    let mut result = vec![];

    for dep_combination in cartesian_product(normalized_deps) {
        for param in params {
            let name = format!("{}[{}]", fixture.name(), stringify_param(param));
            let normalized = NormalizedFixture {
                name: name,
                original_name: fixture.name(),
                param: Some(param),
                dependencies: dep_combination.clone(),
                // ... copy other fixture properties
            };
            result.push(normalized);
        }
    }

    result
}
```

### 2. Normalize a Test Function

```rust
fn normalize_test_function(
    test_fn: &TestFunction,
    fixture_manager: &FixtureManager,
) -> Vec<NormalizedTestFunction> {
    // Get all required fixtures
    let dependencies = test_fn.required_fixtures();

    // Normalize each dependency fixture
    let mut normalized_deps: Vec<Vec<NormalizedFixture>> = vec![];
    for dep_name in dependencies {
        let fixture = fixture_manager.get(dep_name);
        let normalized = normalize_fixture(fixture, fixture_manager);
        normalized_deps.push(normalized);
    }

    // Get test parametrization (from @pytest.mark.parametrize)
    let test_params = test_fn.parametrize_args();

    if test_params.is_empty() && all_deps_have_single_variant(normalized_deps) {
        // No parametrization needed
        return vec![create_normalized_test(test_fn, params=None, deps)];
    }

    // Create cartesian product
    let mut result = vec![];

    for dep_combination in cartesian_product(normalized_deps) {
        for test_param in test_params {
            let name = format!("{}[{}]", test_fn.name(), stringify_params(test_param, dep_combination));
            let normalized = NormalizedTestFunction {
                name: name,
                original_name: test_fn.name(),
                params: test_param,
                fixture_dependencies: dep_combination.clone(),
                // ... copy other properties
            };
            result.push(normalized);
        }
    }

    result
}
```

## Key Data Structures

### NormalizedFixture

```rust
struct NormalizedFixture {
    // Unique name including parameter: "my_fixture[param1]"
    name: String,

    // Original fixture name without parameter: "my_fixture"
    original_name: String,

    // The specific parameter value for this variant (if parametrized)
    param: Option<Value>,

    // Normalized dependencies (already expanded for their params)
    dependencies: Vec<NormalizedFixture>,

    // Original fixture metadata
    scope: FixtureScope,
    auto_use: bool,
    is_generator: bool,
    function: Py<PyAny>,
    function_definition: StmtFunctionDef,
}
```

### NormalizedTestFunction

```rust
struct NormalizedTestFunction {
    // Unique name including all parameters: "test_foo[x=1,y='a']"
    name: String,

    // Original test function name: "test_foo"
    original_name: String,

    // Test-level parameters (from @pytest.mark.parametrize)
    params: HashMap<String, Value>,

    // Normalized fixture dependencies (already expanded)
    fixture_dependencies: Vec<NormalizedFixture>,

    // Original test metadata
    function: Py<PyAny>,
    function_definition: StmtFunctionDef,
    tags: Tags,
}
```

## Handling Reporting

When running tests or reporting failures, we use:
- `normalized_test.name` for uniquely identifying this specific test variant
- `normalized_test.original_name` for grouping and displaying to the user

Example output:
```
test_example[x=1,y='a'] PASSED
test_example[x=1,y='b'] PASSED
test_example[x=2,y='a'] FAILED
test_example[x=2,y='b'] PASSED
```

The user sees it as one test (`test_example`) with different parameter combinations.

## Parameter Stringification

To create unique names, we stringify parameters:

```rust
fn stringify_param(param: &Py<PyAny>) -> String {
    // Use Python's repr() for consistent stringification
    // Handle special cases:
    // - Strings: keep quotes
    // - Numbers: direct string
    // - Objects: use __repr__ or __str__
    // - Collections: recursively stringify
}
```

## Caching and Memoization

To avoid redundant normalization:

```rust
struct NormalizationCache {
    fixtures: HashMap<(String, Vec<String>), Vec<NormalizedFixture>>,
}
```

Key = (fixture_name, sorted_dependency_names)
- Check cache before normalizing
- Store result after normalizing
- Invalidate if fixture definition changes

## Edge Cases

### 1. Circular Dependencies
- Detect during recursive normalization
- Report error to user
- Don't normalize tests that depend on circular fixtures

### 2. Missing Fixtures
- Collect all missing fixtures during normalization
- Report once per test function
- Create placeholder normalized test with error state

### 3. Empty Parameter Lists
- `params=[]` means skip this fixture entirely
- Tests depending on it should also be skipped
- Report as configuration issue

### 4. Scope Interactions
- Session-scoped parametrized fixtures create fewer instances
- Module-scoped fixtures are shared within module
- Function-scoped fixtures are independent per test

## Implementation Steps

1. **Create NormalizedFixture and NormalizedTestFunction structs**
   - Define all fields needed
   - Implement basic construction methods

2. **Implement recursive fixture normalization**
   - Start with fixtures that have no dependencies
   - Work up the dependency tree
   - Handle parametrization at each level

3. **Implement cartesian product logic**
   - Generic function for combining Vec<Vec<T>>
   - Apply to both fixtures and test functions

4. **Integrate into DiscoveredPackageNormalizer**
   - Normalize all fixtures first
   - Then normalize test functions using normalized fixtures

5. **Update test execution**
   - Run normalized tests instead of discovered tests
   - Pass concrete fixture instances (no runtime resolution)

6. **Update reporting**
   - Use original_name for grouping
   - Show full parametrized name in output
   - Aggregate statistics by original_name

## Benefits

1. **Simplicity**: Test execution becomes straightforward - no runtime parametrization logic
2. **Performance**: Pre-compute all variants once instead of during test execution
3. **Debugging**: Each test variant is explicit and traceable
4. **Parallelization**: Each normalized test is independent and can run in parallel
5. **Reporting**: Clear attribution of failures to specific parameter combinations
