use insta_cmd::assert_cmd_snapshot;
use std::process::Command;

fn executable() -> Command {
    Command::new(env!("CARGO_BIN_EXE_karva-language-server"))
}

fn bind_snapshot_settings() -> insta::internals::SettingsBindDropGuard {
    let mut settings = insta::Settings::clone_current();
    settings.add_filter(r"karva \S+", "karva [VERSION]");
    settings.bind_to_scope()
}

#[test]
fn version_is_available_without_starting_lsp_transport() {
    let _settings = bind_snapshot_settings();
    assert_cmd_snapshot!(executable().arg("--version"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    karva [VERSION]

    ----- stderr -----
    ");
}

#[test]
fn help_is_available_without_starting_lsp_transport() {
    let _settings = bind_snapshot_settings();
    assert_cmd_snapshot!(executable().arg("--help"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    Karva language server

    Usage: karva-language-server [OPTIONS]

    Options:
      -V, --version  Print the server version
      -h, --help     Print this help message

    ----- stderr -----
    ");
}

#[test]
fn unexpected_arguments_fail_before_starting_lsp_transport() {
    let _settings = bind_snapshot_settings();
    assert_cmd_snapshot!(executable().arg("--unexpected"), @r"
    success: false
    exit_code: 1
    ----- stdout -----

    ----- stderr -----
    Error: unexpected argument `--unexpected`; the language server communicates over stdio
    ");
}
