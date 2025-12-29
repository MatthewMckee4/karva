pub(crate) mod common;

mod basic;
mod configuration;
mod discovery;
mod extensions;

#[cfg(test)]
#[ctor::ctor]
pub(crate) fn setup() {
    common::create_venv();
}
