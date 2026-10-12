use rstest::rstest;

use crate::common::{TestContext, report_snapshot};

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
    let actual = report_snapshot(&context.measure(calls));
    match outcome {
        "all" => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field             | value
        excluded_lines    | []
        executed_branches | [[2, 3], [2, 4], [4, 5], [4, 7]]
        executed_lines    | [1, 2, 3, 4, 5, 7]
        missing_branches  | []
        missing_lines     | []

        files.sample.py.summary
        field                    | value
        covered_branches         | 4
        covered_lines            | 6
        excluded_lines           | []
        missing_branches         | 0
        missing_lines            | []
        num_branches             | 4
        num_partial_branches     | 0
        num_statements           | 6
        percent_branches_covered | 100.0
        percent_covered          | 100.0

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field                    | value
        covered_branches         | 4
        covered_lines            | 6
        missing_branches         | 0
        num_branches             | 4
        num_partial_branches     | 0
        num_statements           | 6
        percent_branches_covered | 100.0
        percent_covered          | 100.0
        "#),
        "positive" => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field             | value
        excluded_lines    | []
        executed_branches | [[2, 3]]
        executed_lines    | [1, 2, 3]
        missing_branches  | [[2, 4], [4, 5], [4, 7]]
        missing_lines     | [4, 5, 7]

        files.sample.py.summary
        field                    | value
        covered_branches         | 1
        covered_lines            | 3
        excluded_lines           | []
        missing_branches         | 3
        missing_lines            | [4, 5, 7]
        num_branches             | 4
        num_partial_branches     | 1
        num_statements           | 6
        percent_branches_covered | 25.0
        percent_covered          | 40.0

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field                    | value
        covered_branches         | 1
        covered_lines            | 3
        missing_branches         | 3
        num_branches             | 4
        num_partial_branches     | 1
        num_statements           | 6
        percent_branches_covered | 25.0
        percent_covered          | 40.0
        "#),
        "zero" => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field             | value
        excluded_lines    | []
        executed_branches | [[2, 4], [4, 5]]
        executed_lines    | [1, 2, 4, 5]
        missing_branches  | [[2, 3], [4, 7]]
        missing_lines     | [3, 7]

        files.sample.py.summary
        field                    | value
        covered_branches         | 2
        covered_lines            | 4
        excluded_lines           | []
        missing_branches         | 2
        missing_lines            | [3, 7]
        num_branches             | 4
        num_partial_branches     | 2
        num_statements           | 6
        percent_branches_covered | 50.0
        percent_covered          | 60.0

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field                    | value
        covered_branches         | 2
        covered_lines            | 4
        missing_branches         | 2
        num_branches             | 4
        num_partial_branches     | 2
        num_statements           | 6
        percent_branches_covered | 50.0
        percent_covered          | 60.0
        "#),
        _ => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field             | value
        excluded_lines    | []
        executed_branches | [[2, 4], [4, 7]]
        executed_lines    | [1, 2, 4, 7]
        missing_branches  | [[2, 3], [4, 5]]
        missing_lines     | [3, 5]

        files.sample.py.summary
        field                    | value
        covered_branches         | 2
        covered_lines            | 4
        excluded_lines           | []
        missing_branches         | 2
        missing_lines            | [3, 5]
        num_branches             | 4
        num_partial_branches     | 2
        num_statements           | 6
        percent_branches_covered | 50.0
        percent_covered          | 60.0

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field                    | value
        covered_branches         | 2
        covered_lines            | 4
        missing_branches         | 2
        num_branches             | 4
        num_partial_branches     | 2
        num_statements           | 6
        percent_branches_covered | 50.0
        percent_covered          | 60.0
        "#),
    }
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
    let actual = report_snapshot(&context.measure(calls));
    match outcome {
        "all" => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field             | value
        excluded_lines    | []
        executed_branches | [[2, 6], [2, 7]]
        executed_lines    | [1, 2, 6, 7]
        missing_branches  | []
        missing_lines     | []

        files.sample.py.summary
        field                    | value
        covered_branches         | 2
        covered_lines            | 4
        excluded_lines           | []
        missing_branches         | 0
        missing_lines            | []
        num_branches             | 2
        num_partial_branches     | 0
        num_statements           | 4
        percent_branches_covered | 100.0
        percent_covered          | 100.0

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field                    | value
        covered_branches         | 2
        covered_lines            | 4
        missing_branches         | 0
        num_branches             | 2
        num_partial_branches     | 0
        num_statements           | 4
        percent_branches_covered | 100.0
        percent_covered          | 100.0
        "#),
        "first" => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field             | value
        excluded_lines    | []
        executed_branches | [[2, 6]]
        executed_lines    | [1, 2, 6]
        missing_branches  | [[2, 7]]
        missing_lines     | [7]

        files.sample.py.summary
        field                    | value
        covered_branches         | 1
        covered_lines            | 3
        excluded_lines           | []
        missing_branches         | 1
        missing_lines            | [7]
        num_branches             | 2
        num_partial_branches     | 1
        num_statements           | 4
        percent_branches_covered | 50.0
        percent_covered          | 66.66666666666666

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field                    | value
        covered_branches         | 1
        covered_lines            | 3
        missing_branches         | 1
        num_branches             | 2
        num_partial_branches     | 1
        num_statements           | 4
        percent_branches_covered | 50.0
        percent_covered          | 66.66666666666666
        "#),
        "second" => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field             | value
        excluded_lines    | []
        executed_branches | [[2, 6]]
        executed_lines    | [1, 2, 6]
        missing_branches  | [[2, 7]]
        missing_lines     | [7]

        files.sample.py.summary
        field                    | value
        covered_branches         | 1
        covered_lines            | 3
        excluded_lines           | []
        missing_branches         | 1
        missing_lines            | [7]
        num_branches             | 2
        num_partial_branches     | 1
        num_statements           | 4
        percent_branches_covered | 50.0
        percent_covered          | 66.66666666666666

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field                    | value
        covered_branches         | 1
        covered_lines            | 3
        missing_branches         | 1
        num_branches             | 2
        num_partial_branches     | 1
        num_statements           | 4
        percent_branches_covered | 50.0
        percent_covered          | 66.66666666666666
        "#),
        _ => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field             | value
        excluded_lines    | []
        executed_branches | [[2, 7]]
        executed_lines    | [1, 2, 7]
        missing_branches  | [[2, 6]]
        missing_lines     | [6]

        files.sample.py.summary
        field                    | value
        covered_branches         | 1
        covered_lines            | 3
        excluded_lines           | []
        missing_branches         | 1
        missing_lines            | [6]
        num_branches             | 2
        num_partial_branches     | 1
        num_statements           | 4
        percent_branches_covered | 50.0
        percent_covered          | 66.66666666666666

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field                    | value
        covered_branches         | 1
        covered_lines            | 3
        missing_branches         | 1
        num_branches             | 2
        num_partial_branches     | 1
        num_statements           | 4
        percent_branches_covered | 50.0
        percent_covered          | 66.66666666666666
        "#),
    }
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
    let actual = report_snapshot(&context.measure(calls));
    match outcome {
        "all" => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field             | value
        excluded_lines    | []
        executed_branches | [[2, 3], [2, 6], [3, 2], [3, 4]]
        executed_lines    | [1, 2, 3, 4, 6, 7]
        missing_branches  | []
        missing_lines     | []

        files.sample.py.summary
        field                    | value
        covered_branches         | 4
        covered_lines            | 6
        excluded_lines           | []
        missing_branches         | 0
        missing_lines            | []
        num_branches             | 4
        num_partial_branches     | 0
        num_statements           | 6
        percent_branches_covered | 100.0
        percent_covered          | 100.0

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field                    | value
        covered_branches         | 4
        covered_lines            | 6
        missing_branches         | 0
        num_branches             | 4
        num_partial_branches     | 0
        num_statements           | 6
        percent_branches_covered | 100.0
        percent_covered          | 100.0
        "#),
        "exhausted" => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field             | value
        excluded_lines    | []
        executed_branches | [[2, 3], [2, 6], [3, 2]]
        executed_lines    | [1, 2, 3, 6]
        missing_branches  | [[3, 4]]
        missing_lines     | [4, 7]

        files.sample.py.summary
        field                    | value
        covered_branches         | 3
        covered_lines            | 4
        excluded_lines           | []
        missing_branches         | 1
        missing_lines            | [4, 7]
        num_branches             | 4
        num_partial_branches     | 1
        num_statements           | 6
        percent_branches_covered | 75.0
        percent_covered          | 70.0

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field                    | value
        covered_branches         | 3
        covered_lines            | 4
        missing_branches         | 1
        num_branches             | 4
        num_partial_branches     | 1
        num_statements           | 6
        percent_branches_covered | 75.0
        percent_covered          | 70.0
        "#),
        _ => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field             | value
        excluded_lines    | []
        executed_branches | [[2, 3], [3, 4]]
        executed_lines    | [1, 2, 3, 4, 7]
        missing_branches  | [[2, 6], [3, 2]]
        missing_lines     | [6]

        files.sample.py.summary
        field                    | value
        covered_branches         | 2
        covered_lines            | 5
        excluded_lines           | []
        missing_branches         | 2
        missing_lines            | [6]
        num_branches             | 4
        num_partial_branches     | 2
        num_statements           | 6
        percent_branches_covered | 50.0
        percent_covered          | 70.0

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field                    | value
        covered_branches         | 2
        covered_lines            | 5
        missing_branches         | 2
        num_branches             | 4
        num_partial_branches     | 2
        num_statements           | 6
        percent_branches_covered | 50.0
        percent_covered          | 70.0
        "#),
    }
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
    let actual = report_snapshot(&context.measure(calls));
    match outcome {
        "all" => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field             | value
        excluded_lines    | []
        executed_branches | [[2, 3], [2, 6], [3, 2], [3, 4]]
        executed_lines    | [1, 2, 3, 4, 6, 7]
        missing_branches  | []
        missing_lines     | []

        files.sample.py.summary
        field                    | value
        covered_branches         | 4
        covered_lines            | 6
        excluded_lines           | []
        missing_branches         | 0
        missing_lines            | []
        num_branches             | 4
        num_partial_branches     | 0
        num_statements           | 6
        percent_branches_covered | 100.0
        percent_covered          | 100.0

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field                    | value
        covered_branches         | 4
        covered_lines            | 6
        missing_branches         | 0
        num_branches             | 4
        num_partial_branches     | 0
        num_statements           | 6
        percent_branches_covered | 100.0
        percent_covered          | 100.0
        "#),
        "exhausted" => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field             | value
        excluded_lines    | []
        executed_branches | [[2, 3], [2, 6], [3, 2]]
        executed_lines    | [1, 2, 3, 6]
        missing_branches  | [[3, 4]]
        missing_lines     | [4, 7]

        files.sample.py.summary
        field                    | value
        covered_branches         | 3
        covered_lines            | 4
        excluded_lines           | []
        missing_branches         | 1
        missing_lines            | [4, 7]
        num_branches             | 4
        num_partial_branches     | 1
        num_statements           | 6
        percent_branches_covered | 75.0
        percent_covered          | 70.0

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field                    | value
        covered_branches         | 3
        covered_lines            | 4
        missing_branches         | 1
        num_branches             | 4
        num_partial_branches     | 1
        num_statements           | 6
        percent_branches_covered | 75.0
        percent_covered          | 70.0
        "#),
        _ => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field             | value
        excluded_lines    | []
        executed_branches | [[2, 3], [3, 4]]
        executed_lines    | [1, 2, 3, 4, 7]
        missing_branches  | [[2, 6], [3, 2]]
        missing_lines     | [6]

        files.sample.py.summary
        field                    | value
        covered_branches         | 2
        covered_lines            | 5
        excluded_lines           | []
        missing_branches         | 2
        missing_lines            | [6]
        num_branches             | 4
        num_partial_branches     | 2
        num_statements           | 6
        percent_branches_covered | 50.0
        percent_covered          | 70.0

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field                    | value
        covered_branches         | 2
        covered_lines            | 5
        missing_branches         | 2
        num_branches             | 4
        num_partial_branches     | 2
        num_statements           | 6
        percent_branches_covered | 50.0
        percent_covered          | 70.0
        "#),
    }
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
    let actual = report_snapshot(&context.measure(calls));
    if complete {
        insta::assert_snapshot!(actual, @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[9, -6], [9, 10], [15, -12], [15, 16]]
    executed_lines    | [1, 2, 3, 4, 6, 7, 8, 9, 10, 12, 13, 14, 15, 16]
    missing_branches  | []
    missing_lines     | []

    files.sample.py.summary
    field                    | value
    covered_branches         | 4
    covered_lines            | 14
    excluded_lines           | []
    missing_branches         | 0
    missing_lines            | []
    num_branches             | 4
    num_partial_branches     | 0
    num_statements           | 14
    percent_branches_covered | 100.0
    percent_covered          | 100.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 4
    covered_lines            | 14
    missing_branches         | 0
    num_branches             | 4
    num_partial_branches     | 0
    num_statements           | 14
    percent_branches_covered | 100.0
    percent_covered          | 100.0
    "#);
    } else {
        insta::assert_snapshot!(actual, @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[9, -6], [9, 10], [15, 16]]
    executed_lines    | [1, 2, 3, 4, 6, 7, 8, 9, 10, 12, 13, 14, 15, 16]
    missing_branches  | [[15, -12]]
    missing_lines     | []

    files.sample.py.summary
    field                    | value
    covered_branches         | 3
    covered_lines            | 14
    excluded_lines           | []
    missing_branches         | 1
    missing_lines            | []
    num_branches             | 4
    num_partial_branches     | 1
    num_statements           | 14
    percent_branches_covered | 75.0
    percent_covered          | 94.44444444444444

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 3
    covered_lines            | 14
    missing_branches         | 1
    num_branches             | 4
    num_partial_branches     | 1
    num_statements           | 14
    percent_branches_covered | 75.0
    percent_covered          | 94.44444444444444
    "#);
    }
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
    insta::assert_snapshot!(report_snapshot(&context.measure(
        r"
            find([])
            find([[], [-1, 0], [1]])
        "
    )), @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[3, 4], [3, 12], [4, 5], [4, 11], [5, 6], [5, 7], [7, 4], [7, 8]]
    executed_lines    | [1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 12]
    missing_branches  | []
    missing_lines     | []

    files.sample.py.summary
    field                    | value
    covered_branches         | 8
    covered_lines            | 11
    excluded_lines           | []
    missing_branches         | 0
    missing_lines            | []
    num_branches             | 8
    num_partial_branches     | 0
    num_statements           | 11
    percent_branches_covered | 100.0
    percent_covered          | 100.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 8
    covered_lines            | 11
    missing_branches         | 0
    num_branches             | 8
    num_partial_branches     | 0
    num_statements           | 11
    percent_branches_covered | 100.0
    percent_covered          | 100.0
    "#);
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
    insta::assert_snapshot!(report_snapshot(&context.measure(
        r"
            parse('1')
            parse('-3')
            parse('invalid')
        "
    )), @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[10, 11], [10, 12]]
    executed_lines    | [1, 2, 3, 4, 5, 7, 9, 10, 11, 12]
    missing_branches  | []
    missing_lines     | []

    files.sample.py.summary
    field                    | value
    covered_branches         | 2
    covered_lines            | 10
    excluded_lines           | []
    missing_branches         | 0
    missing_lines            | []
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 10
    percent_branches_covered | 100.0
    percent_covered          | 100.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 2
    covered_lines            | 10
    missing_branches         | 0
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 10
    percent_branches_covered | 100.0
    percent_covered          | 100.0
    "#);
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
    let actual = report_snapshot(&context.measure(calls));
    match outcome {
        "all" => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field             | value
        excluded_lines    | []
        executed_branches | [[3, 4], [3, 5], [5, 6], [5, 7]]
        executed_lines    | [1, 2, 3, 4, 5, 6, 7, 8]
        missing_branches  | []
        missing_lines     | []

        files.sample.py.summary
        field                    | value
        covered_branches         | 4
        covered_lines            | 8
        excluded_lines           | []
        missing_branches         | 0
        missing_lines            | []
        num_branches             | 4
        num_partial_branches     | 0
        num_statements           | 8
        percent_branches_covered | 100.0
        percent_covered          | 100.0

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field                    | value
        covered_branches         | 4
        covered_lines            | 8
        missing_branches         | 0
        num_branches             | 4
        num_partial_branches     | 0
        num_statements           | 8
        percent_branches_covered | 100.0
        percent_covered          | 100.0
        "#),
        "guarded" => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field             | value
        excluded_lines    | []
        executed_branches | [[3, 4]]
        executed_lines    | [1, 2, 3, 4]
        missing_branches  | [[3, 5], [5, 6], [5, 7]]
        missing_lines     | [5, 6, 7, 8]

        files.sample.py.summary
        field                    | value
        covered_branches         | 1
        covered_lines            | 4
        excluded_lines           | []
        missing_branches         | 3
        missing_lines            | [5, 6, 7, 8]
        num_branches             | 4
        num_partial_branches     | 1
        num_statements           | 8
        percent_branches_covered | 25.0
        percent_covered          | 41.66666666666667

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field                    | value
        covered_branches         | 1
        covered_lines            | 4
        missing_branches         | 3
        num_branches             | 4
        num_partial_branches     | 1
        num_statements           | 8
        percent_branches_covered | 25.0
        percent_covered          | 41.66666666666667
        "#),
        _ => insta::assert_snapshot!(actual, @r#"
        files.sample.py
        field             | value
        excluded_lines    | []
        executed_branches | [[3, 5], [5, 7]]
        executed_lines    | [1, 2, 3, 5, 7, 8]
        missing_branches  | [[3, 4], [5, 6]]
        missing_lines     | [4, 6]

        files.sample.py.summary
        field                    | value
        covered_branches         | 2
        covered_lines            | 6
        excluded_lines           | []
        missing_branches         | 2
        missing_lines            | [4, 6]
        num_branches             | 4
        num_partial_branches     | 2
        num_statements           | 8
        percent_branches_covered | 50.0
        percent_covered          | 66.66666666666666

        meta
        field   | value
        format  | 2
        version | "karva"

        totals
        field                    | value
        covered_branches         | 2
        covered_lines            | 6
        missing_branches         | 2
        num_branches             | 4
        num_partial_branches     | 2
        num_statements           | 8
        percent_branches_covered | 50.0
        percent_covered          | 66.66666666666666
        "#),
    }
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
    insta::assert_snapshot!(report_snapshot(&context.measure("descend(2)")), @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[2, 3], [2, 5]]
    executed_lines    | [1, 2, 3, 5]
    missing_branches  | []
    missing_lines     | []

    files.sample.py.summary
    field                    | value
    covered_branches         | 2
    covered_lines            | 4
    excluded_lines           | []
    missing_branches         | 0
    missing_lines            | []
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 4
    percent_branches_covered | 100.0
    percent_covered          | 100.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 2
    covered_lines            | 4
    missing_branches         | 0
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 4
    percent_branches_covered | 100.0
    percent_covered          | 100.0
    "#);
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
    insta::assert_snapshot!(report_snapshot(&context.measure(
        r"
            list(generate([]))
            list(generate([0, 1, 2]))
        "
    )), @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[2, 3], [2, 6], [3, 2], [3, 4]]
    executed_lines    | [1, 2, 3, 4, 6]
    missing_branches  | []
    missing_lines     | []

    files.sample.py.summary
    field                    | value
    covered_branches         | 4
    covered_lines            | 5
    excluded_lines           | []
    missing_branches         | 0
    missing_lines            | []
    num_branches             | 4
    num_partial_branches     | 0
    num_statements           | 5
    percent_branches_covered | 100.0
    percent_covered          | 100.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 4
    covered_lines            | 5
    missing_branches         | 0
    num_branches             | 4
    num_partial_branches     | 0
    num_statements           | 5
    percent_branches_covered | 100.0
    percent_covered          | 100.0
    "#);
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
    insta::assert_snapshot!(report_snapshot(&context.measure(
        r"
            asyncio.run(choose(True))
            asyncio.run(choose(False))
        "
    )), @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[5, 6], [5, 7]]
    executed_lines    | [1, 3, 4, 5, 6, 7]
    missing_branches  | []
    missing_lines     | []

    files.sample.py.summary
    field                    | value
    covered_branches         | 2
    covered_lines            | 6
    excluded_lines           | []
    missing_branches         | 0
    missing_lines            | []
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 6
    percent_branches_covered | 100.0
    percent_covered          | 100.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 2
    covered_lines            | 6
    missing_branches         | 0
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 6
    percent_branches_covered | 100.0
    percent_covered          | 100.0
    "#);
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
    insta::assert_snapshot!(report_snapshot(&context.measure(
        r"
            choose(True)
            choose(False)
        "
    )), @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[4, 5], [4, 7]]
    executed_lines    | [1, 2, 3, 4, 5, 7, 8]
    missing_branches  | []
    missing_lines     | []

    files.sample.py.summary
    field                    | value
    covered_branches         | 2
    covered_lines            | 7
    excluded_lines           | []
    missing_branches         | 0
    missing_lines            | []
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 7
    percent_branches_covered | 100.0
    percent_covered          | 100.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 2
    covered_lines            | 7
    missing_branches         | 0
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 7
    percent_branches_covered | 100.0
    percent_covered          | 100.0
    "#);
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
    insta::assert_snapshot!(report_snapshot(&context.measure(
        r"
            choose(True, '1')
            choose(False, '1')
            choose(False, 'invalid')
        "
    )), @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[4, 5], [4, 9]]
    executed_lines    | [1, 2, 3, 4, 5, 6, 7, 9, 11, 12]
    missing_branches  | []
    missing_lines     | []

    files.sample.py.summary
    field                    | value
    covered_branches         | 2
    covered_lines            | 10
    excluded_lines           | []
    missing_branches         | 0
    missing_lines            | []
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 10
    percent_branches_covered | 100.0
    percent_covered          | 100.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 2
    covered_lines            | 10
    missing_branches         | 0
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 10
    percent_branches_covered | 100.0
    percent_covered          | 100.0
    "#);
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
    let actual = report_snapshot(&context.measure(calls));
    if complete {
        insta::assert_snapshot!(actual, @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[5, 4], [5, 6]]
    executed_lines    | [1, 3, 4, 5, 6, 7]
    missing_branches  | []
    missing_lines     | []

    files.sample.py.summary
    field                    | value
    covered_branches         | 2
    covered_lines            | 6
    excluded_lines           | []
    missing_branches         | 0
    missing_lines            | []
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 6
    percent_branches_covered | 100.0
    percent_covered          | 100.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 2
    covered_lines            | 6
    missing_branches         | 0
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 6
    percent_branches_covered | 100.0
    percent_covered          | 100.0
    "#);
    } else {
        insta::assert_snapshot!(actual, @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[5, 6]]
    executed_lines    | [1, 3, 4, 5, 6]
    missing_branches  | [[5, 4]]
    missing_lines     | [7]

    files.sample.py.summary
    field                    | value
    covered_branches         | 1
    covered_lines            | 5
    excluded_lines           | []
    missing_branches         | 1
    missing_lines            | [7]
    num_branches             | 2
    num_partial_branches     | 1
    num_statements           | 6
    percent_branches_covered | 50.0
    percent_covered          | 75.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 1
    covered_lines            | 5
    missing_branches         | 1
    num_branches             | 2
    num_partial_branches     | 1
    num_statements           | 6
    percent_branches_covered | 50.0
    percent_covered          | 75.0
    "#);
    }
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
    insta::assert_snapshot!(report_snapshot(&context.measure(
        r"
            asyncio.run(choose([]))
            asyncio.run(choose([0]))
            asyncio.run(choose([1]))
        "
    )), @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[4, -3], [4, 5], [8, 9], [8, 12], [9, 8], [9, 10]]
    executed_lines    | [1, 3, 4, 5, 7, 8, 9, 10, 12, 13]
    missing_branches  | []
    missing_lines     | []

    files.sample.py.summary
    field                    | value
    covered_branches         | 6
    covered_lines            | 10
    excluded_lines           | []
    missing_branches         | 0
    missing_lines            | []
    num_branches             | 6
    num_partial_branches     | 0
    num_statements           | 10
    percent_branches_covered | 100.0
    percent_covered          | 100.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 6
    covered_lines            | 10
    missing_branches         | 0
    num_branches             | 6
    num_partial_branches     | 0
    num_statements           | 10
    percent_branches_covered | 100.0
    percent_covered          | 100.0
    "#);
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
    insta::assert_snapshot!(report_snapshot(&context.measure(
        r"
            asyncio.run(choose(True))
            asyncio.run(choose(False))
        "
    )), @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[6, 5], [6, 7]]
    executed_lines    | [1, 2, 4, 5, 6, 7, 8]
    missing_branches  | []
    missing_lines     | []

    files.sample.py.summary
    field                    | value
    covered_branches         | 2
    covered_lines            | 7
    excluded_lines           | []
    missing_branches         | 0
    missing_lines            | []
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 7
    percent_branches_covered | 100.0
    percent_covered          | 100.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 2
    covered_lines            | 7
    missing_branches         | 0
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 7
    percent_branches_covered | 100.0
    percent_covered          | 100.0
    "#);
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
    insta::assert_snapshot!(report_snapshot(&context.measure(
        r"
            create(True)
            create(False)
        "
    )), @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[8, -5], [8, 9]]
    executed_lines    | [1, 2, 4, 5, 6, 7, 8, 9, 10]
    missing_branches  | []
    missing_lines     | []

    files.sample.py.summary
    field                    | value
    covered_branches         | 2
    covered_lines            | 9
    excluded_lines           | []
    missing_branches         | 0
    missing_lines            | []
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 9
    percent_branches_covered | 100.0
    percent_covered          | 100.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 2
    covered_lines            | 9
    missing_branches         | 0
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 9
    percent_branches_covered | 100.0
    percent_covered          | 100.0
    "#);
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
    let actual = report_snapshot(&context.measure(calls));
    if complete {
        insta::assert_snapshot!(actual, @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[2, -1], [2, 3]]
    executed_lines    | [1, 2, 3]
    missing_branches  | []
    missing_lines     | []

    files.sample.py.summary
    field                    | value
    covered_branches         | 2
    covered_lines            | 3
    excluded_lines           | []
    missing_branches         | 0
    missing_lines            | []
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 3
    percent_branches_covered | 100.0
    percent_covered          | 100.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 2
    covered_lines            | 3
    missing_branches         | 0
    num_branches             | 2
    num_partial_branches     | 0
    num_statements           | 3
    percent_branches_covered | 100.0
    percent_covered          | 100.0
    "#);
    } else {
        insta::assert_snapshot!(actual, @r#"
    files.sample.py
    field             | value
    excluded_lines    | []
    executed_branches | [[2, -1]]
    executed_lines    | [1, 2]
    missing_branches  | [[2, 3]]
    missing_lines     | [3]

    files.sample.py.summary
    field                    | value
    covered_branches         | 1
    covered_lines            | 2
    excluded_lines           | []
    missing_branches         | 1
    missing_lines            | [3]
    num_branches             | 2
    num_partial_branches     | 1
    num_statements           | 3
    percent_branches_covered | 50.0
    percent_covered          | 60.0

    meta
    field   | value
    format  | 2
    version | "karva"

    totals
    field                    | value
    covered_branches         | 1
    covered_lines            | 2
    missing_branches         | 1
    num_branches             | 2
    num_partial_branches     | 1
    num_statements           | 3
    percent_branches_covered | 50.0
    percent_covered          | 60.0
    "#);
    }
}
