use ruff_source_file::LineIndex;

use crate::{
    discovery::ModuleType, extensions::fixtures::NormalizedFixture, name::ModulePath,
    normalize::models::NormalizedTestFunction,
};

#[derive(Debug)]
pub struct NormalizedModule {
    pub(crate) path: ModulePath,

    pub(crate) test_functions: Vec<NormalizedTestFunction>,

    pub(crate) fixtures: Vec<NormalizedFixture>,

    pub(crate) type_: ModuleType,

    pub(crate) source_text: String,

    pub(crate) line_index: LineIndex,
}
