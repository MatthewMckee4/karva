# Installation

Install [Karva](https://pypi.org/project/karva/) as a development dependency:

```bash
uv add --dev karva
uv run karva test
```

For a global installation, use `uv tool install karva@latest`.
Or install with pip:

```bash
pip install karva
```

Check the installed version:

```bash
uv run karva version
```

## Shell autocompletion

Generate completions with `uv run karva generate-shell-completion <SHELL>`.
Supported shells: Bash, Zsh, fish, Elvish, PowerShell, and Nushell.

!!! tip

    You can run `echo $SHELL` to help you determine your shell.

Run the command for your shell:

=== "Bash"

    ```bash
    echo 'eval "$(uv run karva generate-shell-completion bash)"' >> ~/.bashrc
    ```

=== "Zsh"

    ```bash
    echo 'eval "$(uv run karva generate-shell-completion zsh)"' >> ~/.zshrc
    ```

=== "fish"

    ```bash
    echo 'uv run karva generate-shell-completion fish | source' > ~/.config/fish/completions/karva.fish
    ```

=== "Elvish"

    ```bash
    echo 'eval (uv run karva generate-shell-completion elvish | slurp)' >> ~/.elvish/rc.elv
    ```

=== "PowerShell / pwsh"

    ```powershell
    if (!(Test-Path -Path $PROFILE)) {
      New-Item -ItemType File -Path $PROFILE -Force
    }
    Add-Content -Path $PROFILE -Value '(& uv run karva generate-shell-completion powershell) | Out-String | Invoke-Expression'
    ```

=== "Nushell"

    ```nu
    mkdir ($nu.user-autoload-dirs | first)
    uv run karva generate-shell-completion nushell | save --force ($nu.user-autoload-dirs | first | path join karva.nu)
    ```

Then restart the shell or source the shell config file.
