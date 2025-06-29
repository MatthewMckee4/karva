use karva::{PyTag, PyTags};
use karva_core::testing::setup;
use pyo3::{
    ffi::c_str,
    prelude::*,
    types::{PyDict, PyType},
};

#[test]
fn test_parametrize_single_arg() {
    setup();

    Python::with_gil(|py| {
        let locals = PyDict::new(py);
        Python::run(
            py,
            c_str!("import karva;tags = karva.tags"),
            None,
            Some(&locals),
        )
        .unwrap();

        let binding = locals.get_item("tags").unwrap().unwrap();
        let cls = binding.downcast::<PyType>().unwrap();

        let arg_names = py.eval(c_str!("'a'"), None, None).unwrap();
        let arg_values = py.eval(c_str!("[1, 2, 3]"), None, None).unwrap();
        let tags = PyTags::parametrize(cls, &arg_names, &arg_values).unwrap();
        assert_eq!(tags.inner.len(), 1);
        assert_eq!(tags.inner[0].name(), "parametrize");
        let PyTag::Parametrize {
            arg_names,
            arg_values,
        } = &tags.inner[0];
        assert_eq!(arg_names, &vec!["a"]);
        assert_eq!(arg_values.len(), 3);
        assert_eq!(
            arg_values[0].first().unwrap().extract::<i32>(py).unwrap(),
            1
        );
        assert_eq!(
            arg_values[1].first().unwrap().extract::<i32>(py).unwrap(),
            2
        );
        assert_eq!(
            arg_values[2].first().unwrap().extract::<i32>(py).unwrap(),
            3
        );
    });
}

#[test]
fn test_parametrize_multiple_args() {
    setup();

    Python::with_gil(|py| {
        let locals = PyDict::new(py);
        Python::run(
            py,
            c_str!("import karva;tags = karva.tags"),
            None,
            Some(&locals),
        )
        .unwrap();

        let binding = locals.get_item("tags").unwrap().unwrap();
        let cls = binding.downcast::<PyType>().unwrap();

        let arg_names = py.eval(c_str!("('a', 'b')"), None, None).unwrap();
        let arg_values = py.eval(c_str!("[[1, 2], [3, 4]]"), None, None).unwrap();
        let tags = PyTags::parametrize(cls, &arg_names, &arg_values).unwrap();
        assert_eq!(tags.inner.len(), 1);
        assert_eq!(tags.inner[0].name(), "parametrize");
        let PyTag::Parametrize {
            arg_names,
            arg_values,
        } = &tags.inner[0];
        assert_eq!(arg_names, &vec!["a", "b"]);
        assert_eq!(arg_values.len(), 2);
        assert_eq!(arg_values[0].len(), 2);
        assert_eq!(arg_values[0][0].extract::<i32>(py).unwrap(), 1);
        assert_eq!(arg_values[0][1].extract::<i32>(py).unwrap(), 2);
        assert_eq!(arg_values[1].len(), 2);
        assert_eq!(arg_values[1][0].extract::<i32>(py).unwrap(), 3);
        assert_eq!(arg_values[1][1].extract::<i32>(py).unwrap(), 4);
    });
}
