"""Small stdio LSP peer shared by editor smoke tests and measurements."""

import json
import subprocess


class LspClient:
    """One synchronous stdio peer; server stderr cannot fill a pipe."""

    def __init__(self, command, cwd, stderr):
        """Launch a peer with stderr owned by the caller's context manager."""
        self.stderr = stderr
        self.process = subprocess.Popen(
            command,
            cwd=cwd,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=self.stderr,
        )
        self.next_id = 0
        self.notifications = []

    def send(self, message):
        """Write one complete LSP frame."""
        body = json.dumps({"jsonrpc": "2.0", **message}).encode()
        self.process.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
        self.process.stdin.flush()

    def receive(self):
        """Read one complete frame, reporting early process failure."""
        headers = {}
        while (line := self.process.stdout.readline()) not in (b"\r\n", b"\n"):
            if not line:
                self.stderr.seek(0)
                raise RuntimeError(
                    f"Server closed stdout: {self.stderr.read().decode(errors='replace')}"
                )
            key, value = line.decode().split(":", 1)
            headers[key.lower()] = value.strip()
        return json.loads(self.process.stdout.read(int(headers["content-length"])))

    def request(self, method, params):
        """Wait for a matching response while accepting file-watcher registration."""
        self.next_id += 1
        request_id = self.next_id
        self.send({"id": request_id, "method": method, "params": params})
        while True:
            message = self.receive()
            if "method" in message and "id" in message:
                # Dynamic file watching registration is accepted by this fixture.
                if message["method"] != "client/registerCapability":
                    raise RuntimeError(f"Unexpected server request: {message}")
                self.send({"id": message["id"], "result": None})
            elif message.get("id") == request_id:
                if "error" in message:
                    raise RuntimeError(message["error"])
                return message.get("result")
            else:
                self.notifications.append(message)

    def notify(self, method, params):
        """Send a notification without expecting a response."""
        self.send({"method": method, "params": params})

    def close(self):
        """Reap the subprocess after either successful shutdown or failure."""
        self.process.kill()
        self.process.wait()
        self.process.stdin.close()
        self.process.stdout.close()
