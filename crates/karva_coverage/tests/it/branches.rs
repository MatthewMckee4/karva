use rstest::rstest;

use crate::common::TestContext;

#[rstest]
fn if_elif_else(#[values("all", "positive", "zero", "negative")] outcome: &str) {
    let context = TestContext::new(
        r"
        def choose(value):
            if value > 0:
                return 'positive'
            elif value == 0:
                return 'zero'
            else:
                return 'negative'
    ",
    );
    let calls = match outcome {
        "positive" => "choose(1)",
        "zero" => "choose(0)",
        "negative" => "choose(-1)",
        _ => {
            r"
            choose(1)
            choose(0)
            choose(-1)
        "
        }
    };
    insta::with_settings!({snapshot_suffix => outcome}, {
        insta::assert_json_snapshot!(context.measure(calls));
    });
}

#[rstest]
fn multiline_boolean_condition(#[values("all", "first", "second", "neither")] outcome: &str) {
    let context = TestContext::new(
        r"
        def choose(first, second):
            if (
                first
                or second
            ):
                return 1
            return 0
    ",
    );
    let calls = match outcome {
        "first" => "choose(True, False)",
        "second" => "choose(False, True)",
        "neither" => "choose(False, False)",
        _ => {
            r"
            choose(True, False)
            choose(False, True)
            choose(False, False)
        "
        }
    };
    insta::with_settings!({snapshot_suffix => outcome}, {
        insta::assert_json_snapshot!(context.measure(calls));
    });
}

#[rstest]
fn for_else_return(#[values("all", "exhausted", "broken")] outcome: &str) {
    let context = TestContext::new(
        r"
        def choose(values):
            for value in values:
                if value:
                    break
            else:
                return []
            return [1]
    ",
    );
    let calls = match outcome {
        "exhausted" => "choose([0])",
        "broken" => "choose([1])",
        _ => {
            r"
            choose([])
            choose([0])
            choose([1])
        "
        }
    };
    insta::with_settings!({snapshot_suffix => outcome}, {
        insta::assert_json_snapshot!(context.measure(calls));
    });
}

#[rstest]
fn while_else_return(#[values("all", "exhausted", "broken")] outcome: &str) {
    let context = TestContext::new(
        r"
        def choose(values):
            while values:
                if values.pop():
                    break
            else:
                return []
            return [1]
    ",
    );
    let calls = match outcome {
        "exhausted" => "choose([0])",
        "broken" => "choose([1])",
        _ => {
            r"
            choose([])
            choose([0])
            choose([1])
        "
        }
    };
    insta::with_settings!({snapshot_suffix => outcome}, {
        insta::assert_json_snapshot!(context.measure(calls));
    });
}

#[rstest]
fn decorated_implicit_returns(#[values(false, true)] complete: bool) {
    let context = TestContext::new(
        r"
        def wrap(function):
            def wrapper(*args):
                return function(*args)
            return wrapper

        @wrap
        @wrap
        def final_loop(values):
            for value in values:
                assert value > 0

        @wrap
        @wrap
        def final_condition(enabled):
            if enabled:
                pass
    ",
    );
    let calls = if complete {
        r"
        final_loop([])
        final_loop([1])
        final_condition(False)
        final_condition(True)
    "
    } else {
        r"
        final_loop([1])
        final_condition(True)
    "
    };
    insta::with_settings!({snapshot_suffix => complete.to_string()}, {
        insta::assert_json_snapshot!(context.measure(calls));
    });
}

#[test]
fn nested_loops_break_and_continue() {
    let context = TestContext::new(
        r"
        def find(rows):
            result = []
            for row in rows:
                for value in row:
                    if value < 0:
                        continue
                    if value:
                        result.append(value)
                        break
                else:
                    result.append(None)
            return result
    ",
    );
    insta::assert_json_snapshot!(context.measure(
        r"
        find([])
        find([[], [-1, 0], [1]])
    "
    ));
}

#[test]
fn try_except_else_finally() {
    let context = TestContext::new(
        r"
        def parse(value):
            try:
                result = int(value)
            except ValueError:
                result = -1
            else:
                result += 1
            finally:
                result += 2
            if result > 0:
                return result
            return 0
    ",
    );
    insta::assert_json_snapshot!(context.measure(
        r"
        parse('1')
        parse('-3')
        parse('invalid')
    "
    ));
}

#[rstest]
fn match_with_guard(#[values("all", "guarded", "fallback")] outcome: &str) {
    let context = TestContext::new(
        r"
        def choose(value):
            match value:
                case int() if value > 0:
                    return 'positive'
                case 0:
                    return 'zero'
                case _:
                    return 'other'
    ",
    );
    let calls = match outcome {
        "guarded" => "choose(1)",
        "fallback" => "choose('other')",
        _ => {
            r"
            choose(1)
            choose(0)
            choose(-1)
            choose('other')
        "
        }
    };
    insta::with_settings!({snapshot_suffix => outcome}, {
        insta::assert_json_snapshot!(context.measure(calls));
    });
}

#[test]
fn recursive_function() {
    let context = TestContext::new(
        r"
        def descend(value):
            if value:
                descend(value - 1)
            else:
                return 0
    ",
    );
    insta::assert_json_snapshot!(context.measure("descend(2)"));
}

#[test]
fn generator_resumption() {
    let context = TestContext::new(
        r"
        def generate(values):
            for value in values:
                if value:
                    yield value
            else:
                yield None
    ",
    );
    insta::assert_json_snapshot!(context.measure(
        r"
        list(generate([]))
        list(generate([0, 1, 2]))
    "
    ));
}

#[test]
fn async_function() {
    let context = TestContext::new(
        r"
        import asyncio

        async def choose(flag):
            await asyncio.sleep(0)
            if flag:
                return 1
            return 0
    ",
    );
    insta::assert_json_snapshot!(context.measure(
        r"
        asyncio.run(choose(True))
        asyncio.run(choose(False))
    "
    ));
}

#[test]
fn try_finally_fallthrough() {
    let context = TestContext::new(
        r"
        def choose(flag):
            value = 0
            try:
                if flag:
                    value = 1
            finally:
                value += 2
            return value
    ",
    );
    insta::assert_json_snapshot!(context.measure(
        r"
        choose(True)
        choose(False)
    "
    ));
}

#[test]
fn try_else_fallthrough() {
    let context = TestContext::new(
        r"
        def choose(flag, value):
            try:
                result = int(value)
                if flag:
                    result += 1
            except ValueError:
                result = 0
            else:
                result += 2
            finally:
                result += 3
            return result
    ",
    );
    insta::assert_json_snapshot!(context.measure(
        r"
        choose(True, '1')
        choose(False, '1')
        choose(False, 'invalid')
    "
    ));
}

#[rstest]
fn with_statement(#[values(false, true)] complete: bool) {
    let context = TestContext::new(
        r"
        from contextlib import nullcontext

        def choose(flag):
            with nullcontext():
                if flag:
                    return 1
            return 0
    ",
    );
    let calls = if complete {
        r"
        choose(True)
        choose(False)
    "
    } else {
        "choose(True)"
    };
    insta::with_settings!({snapshot_suffix => complete.to_string()}, {
        insta::assert_json_snapshot!(context.measure(calls));
    });
}

#[test]
fn async_for_else() {
    let context = TestContext::new(
        r"
        import asyncio

        async def generate(values):
            for value in values:
                yield value

        async def choose(values):
            async for value in generate(values):
                if value:
                    break
            else:
                return []
            return [1]
    ",
    );
    insta::assert_json_snapshot!(context.measure(
        r"
        asyncio.run(choose([]))
        asyncio.run(choose([0]))
        asyncio.run(choose([1]))
    "
    ));
}

#[test]
fn async_with_statement() {
    let context = TestContext::new(
        r"
        import asyncio
        from contextlib import nullcontext

        async def choose(flag):
            async with nullcontext():
                if flag:
                    return 1
            return 0
    ",
    );
    insta::assert_json_snapshot!(context.measure(
        r"
        asyncio.run(choose(True))
        asyncio.run(choose(False))
    "
    ));
}

#[test]
fn decorated_class_body() {
    let context = TestContext::new(
        r"
        def wrap(cls):
            return cls

        def create(flag):
            @wrap
            @wrap
            class Example:
                if flag:
                    value = 1
            return Example
    ",
    );
    insta::assert_json_snapshot!(context.measure(
        r"
        create(True)
        create(False)
    "
    ));
}

#[rstest]
fn module_fallthrough(#[values(false, true)] complete: bool) {
    let context = TestContext::new(
        r"
        enabled = globals().get('enabled', False)
        if enabled:
            value = 1
    ",
    );
    let calls = if complete {
        r"
        from pathlib import Path
        enabled = True
        exec(compile(Path(__file__).read_text(), __file__, 'exec'))
    "
    } else {
        ""
    };
    insta::with_settings!({snapshot_suffix => complete.to_string()}, {
        insta::assert_json_snapshot!(context.measure(calls));
    });
}
