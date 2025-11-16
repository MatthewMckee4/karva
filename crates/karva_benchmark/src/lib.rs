use camino::Utf8PathBuf;

pub mod criterion;
pub mod walltime;

pub static TRUE_ASSERTIONS: TestFile = TestFile::new(
    "test_true_assertions.py",
    include_str!("../resources/test_true_assertions.py"),
);

pub static MATH: TestFile =
    TestFile::new("test_math.py", include_str!("../resources/test_math.py"));

pub static STRING_CONCATENATION: TestFile = TestFile::new(
    "test_string_concatenation.py",
    include_str!("../resources/test_string_concatenation.py"),
);

pub static LARGE_SUMMATION: TestFile = TestFile::new(
    "test_large_summation.py",
    include_str!("../resources/test_large_summation.py"),
);

pub static LARGE_LIST_COMPREHENSION: TestFile = TestFile::new(
    "test_large_list_comprehension.py",
    include_str!("../resources/test_large_list_comprehension.py"),
);

pub static FIXTURES: TestFile = TestFile::new(
    "test_fixtures.py",
    include_str!("../resources/test_fixtures.py"),
);

pub static PARAMETRIZE: TestFile = TestFile::new(
    "test_parametrize.py",
    include_str!("../resources/test_parametrize.py"),
);

/// Relative size of a test case. Benchmarks can use it to configure the time for how long a benchmark should run to get stable results.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum TestCaseSpeed {
    /// A test case that is fast to run
    Fast,

    /// A normal test case
    Normal,

    /// A slow test case
    Slow,
}

#[derive(Debug, Clone)]
pub struct TestCase {
    file: TestFile,
}

impl TestCase {
    pub const fn new(file: TestFile) -> Self {
        Self { file }
    }

    pub const fn code(&self) -> &str {
        self.file.code
    }

    pub const fn name(&self) -> &str {
        self.file.name
    }

    pub fn path(&self) -> Utf8PathBuf {
        Utf8PathBuf::from(file!())
            .parent()
            .expect("Failed to get parent of file")
            .parent()
            .expect("Failed to get parent of file")
            .join("resources")
            .join(self.name())
    }
}

#[derive(Debug, Clone)]
pub struct TestFile {
    name: &'static str,
    code: &'static str,
}

impl TestFile {
    pub const fn new(name: &'static str, code: &'static str) -> Self {
        Self { name, code }
    }

    pub const fn code(&self) -> &str {
        self.code
    }

    pub const fn name(&self) -> &str {
        self.name
    }
}
