use std::process::Command;

fn executable() -> Command {
    Command::new(env!("CARGO_BIN_EXE_karva-language-server"))
}

#[test]
fn version_is_available_without_starting_lsp_transport() {
    let output = executable()
        .arg("--version")
        .output()
        .expect("language-server executable should run");

    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        format!("karva {}\n", karva_version::version()).as_bytes()
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn help_is_available_without_starting_lsp_transport() {
    let output = executable()
        .arg("--help")
        .output()
        .expect("language-server executable should run");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Usage: karva-language-server"));
    assert!(output.stderr.is_empty());
}

#[test]
fn unexpected_arguments_fail_before_starting_lsp_transport() {
    let output = executable()
        .arg("--unexpected")
        .output()
        .expect("language-server executable should run");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument"));
}
