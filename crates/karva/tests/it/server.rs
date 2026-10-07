use std::io::{BufRead, BufReader, BufWriter, Write};
use std::process::Stdio;

use anyhow::{Context, bail};
use lsp_server::{Message, Notification, Request, Response};
use serde_json::json;

use crate::common::TestContext;

fn send_message(writer: &mut impl Write, message: &Message) -> anyhow::Result<()> {
    message.write(writer)?;
    writer.flush()?;
    Ok(())
}

fn receive_response(reader: &mut impl BufRead) -> anyhow::Result<Response> {
    match Message::read(reader)? {
        Some(Message::Response(response)) => Ok(response),
        Some(message) => bail!("expected LSP response, received {message:?}"),
        None => bail!("language server closed stdout before responding"),
    }
}

#[test]
fn server_subcommand_completes_lsp_lifecycle() -> anyhow::Result<()> {
    let context = TestContext::new();
    let mut command = context.karva_command_in(context.root());
    let mut child = command
        .arg("server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = BufWriter::new(child.stdin.take().context("server stdin unavailable")?);
    let mut stdout = BufReader::new(child.stdout.take().context("server stdout unavailable")?);

    send_message(
        &mut stdin,
        &Message::Request(Request {
            id: 1.into(),
            method: "initialize".to_owned(),
            params: json!({
                "capabilities": {},
                "workspaceFolders": [],
            }),
        }),
    )?;
    let initialize = receive_response(&mut stdout)?;
    let initialize_result = initialize
        .response_result
        .as_ref()
        .map_err(|error| anyhow::anyhow!("initialize failed: {error:?}"))?;

    send_message(
        &mut stdin,
        &Message::Notification(Notification {
            method: "initialized".to_owned(),
            params: json!({}),
        }),
    )?;
    send_message(
        &mut stdin,
        &Message::Request(Request {
            id: 2.into(),
            method: "shutdown".to_owned(),
            params: serde_json::Value::Null,
        }),
    )?;
    let shutdown = receive_response(&mut stdout)?;
    let shutdown_result = shutdown
        .response_result
        .as_ref()
        .map_err(|error| anyhow::anyhow!("shutdown failed: {error:?}"))?;

    send_message(
        &mut stdin,
        &Message::Notification(Notification {
            method: "exit".to_owned(),
            params: serde_json::Value::Null,
        }),
    )?;
    drop(stdin);
    drop(stdout);
    let output = child.wait_with_output()?;

    let observed = json!({
        "initialize": {
            "id": initialize.id.to_string(),
            "positionEncoding": initialize_result["capabilities"]["positionEncoding"],
            "serverName": initialize_result["serverInfo"]["name"],
        },
        "shutdown": {
            "id": shutdown.id.to_string(),
            "result": shutdown_result,
        },
    });
    insta::assert_snapshot!(format!(
        "success: {}\nexit_code: {:?}\n----- stdout -----\n{}\n----- stderr -----\n{}",
        output.status.success(),
        output.status.code(),
        serde_json::to_string_pretty(&observed)?,
        String::from_utf8(output.stderr)?,
    ), @r#"
    success: true
    exit_code: Some(0)
    ----- stdout -----
    {
      "initialize": {
        "id": "1",
        "positionEncoding": "utf-16",
        "serverName": "karva"
      },
      "shutdown": {
        "id": "2",
        "result": null
      }
    }
    ----- stderr -----
    "#);
    Ok(())
}

#[test]
fn server_reports_initialization_failure_with_stdin_open() -> anyhow::Result<()> {
    let context = TestContext::new();
    let mut command = context.karva_command_in(context.root());
    let mut child = command
        .arg("server")
        .env_remove("RUST_LOG")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = BufWriter::new(child.stdin.take().context("server stdin unavailable")?);
    send_message(
        &mut stdin,
        &Message::Request(Request {
            id: 1.into(),
            method: "initialize".to_owned(),
            params: json!({
                "capabilities": {},
                "initializationOptions": {"pythonVersion": "invalid"},
            }),
        }),
    )?;
    // Keep the editor's stdin open: initialization errors must not join its reader.
    let output = child.wait_with_output()?;
    drop(stdin);
    let stdout = String::from_utf8(output.stdout)?;
    let stderr = String::from_utf8(output.stderr)?;
    insta::with_settings!({filters => [(r"\d{4}-\d{2}-\d{2}T\S+", "[TIME]")]}, {
        insta::assert_snapshot!(format!(
            "success: {}\nexit_code: {:?}\n----- stdout -----\n{}\n----- stderr -----\n{}",
            output.status.success(), output.status.code(),
            stdout, stderr,
        ), @"
        success: false
        exit_code: Some(2)
        ----- stdout -----

        ----- stderr -----
        [TIME] ERROR karva_language_server::runtime: Karva language server failed error=failed to initialize language server: Invalid python version `invalid`: expected `major.minor`
        karva failed
          Cause: failed to initialize language server
          Cause: Invalid python version `invalid`: expected `major.minor`
        ");
    });
    Ok(())
}

#[test]
fn server_reports_main_loop_failure_with_stdin_open() -> anyhow::Result<()> {
    let context = TestContext::new();
    let mut command = context.karva_command_in(context.root());
    let mut child = command
        .arg("server")
        .env_remove("RUST_LOG")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = BufWriter::new(child.stdin.take().context("server stdin unavailable")?);
    let mut stdout = BufReader::new(child.stdout.take().context("server stdout unavailable")?);
    send_message(
        &mut stdin,
        &Message::Request(Request {
            id: 1.into(),
            method: "initialize".to_owned(),
            params: json!({"capabilities": {}}),
        }),
    )?;
    let initialize = receive_response(&mut stdout)?;
    assert!(initialize.response_result.is_ok());
    send_message(
        &mut stdin,
        &Message::Notification(Notification {
            method: "initialized".to_owned(),
            params: json!({}),
        }),
    )?;
    send_message(
        &mut stdin,
        &Message::Notification(Notification {
            method: "exit".to_owned(),
            params: serde_json::Value::Null,
        }),
    )?;
    // The old error path joined the reader indefinitely with stdin still open.
    let mut remaining = String::new();
    std::io::Read::read_to_string(&mut stdout, &mut remaining)?;
    let output = child.wait_with_output()?;
    drop(stdin);
    let stderr = String::from_utf8(output.stderr)?;
    insta::with_settings!({filters => [(r"\d{4}-\d{2}-\d{2}T\S+", "[TIME]")]}, {
        insta::assert_snapshot!(format!(
            "success: {}\nexit_code: {:?}\n----- stdout -----\ninitialize_id: {}\n{}\n----- stderr -----\n{}",
            output.status.success(), output.status.code(), initialize.id, remaining, stderr,
        ), @"
        success: false
        exit_code: Some(2)
        ----- stdout -----
        initialize_id: 1

        ----- stderr -----
        [TIME] ERROR karva_language_server::runtime: Karva language server failed error=language-server event loop failed: received exit notification before shutdown request
        karva failed
          Cause: language-server event loop failed
          Cause: received exit notification before shutdown request
        ");
    });
    Ok(())
}
