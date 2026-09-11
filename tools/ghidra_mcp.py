#!/usr/bin/env python3
"""Small SSE MCP client. Read-only analysis commands are recorded as evidence."""
import argparse
import json
import queue
import threading
import urllib.request
from pathlib import Path


class Client:
    def __init__(self, base="http://127.0.0.1:8081"):
        self.base = base
        self.queue = queue.Queue()
        self.stream = urllib.request.urlopen(base + "/sse", timeout=180)
        threading.Thread(target=self._read, daemon=True).start()
        self.endpoint = self.queue.get(timeout=15)
        self.serial = 0
        self.request("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                    "clientInfo": {"name": "phantom-linux-analysis", "version": "0.1"}})
        self.post({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def _read(self):
        for line in self.stream:
            line = line.decode().strip()
            if line.startswith("data: "):
                self.queue.put(line[6:])

    def post(self, obj):
        req = urllib.request.Request(self.base + self.endpoint, data=json.dumps(obj).encode(),
                                     headers={"Content-Type": "application/json"})
        urllib.request.urlopen(req, timeout=180).read()

    def request(self, method, params):
        self.serial += 1
        self.post({"jsonrpc": "2.0", "id": self.serial, "method": method, "params": params})
        while True:
            response = json.loads(self.queue.get(timeout=180))
            if response.get("id") == self.serial:
                if "error" in response:
                    raise RuntimeError(response["error"])
                return response["result"]

    def call(self, name, arguments):
        return self.request("tools/call", {"name": name, "arguments": arguments})


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("tool")
    parser.add_argument("arguments", nargs="?", default="{}")
    parser.add_argument("--out")
    args = parser.parse_args()
    client = Client()
    if args.tool == "decompile-batch":
        for address in json.loads(args.arguments):
            result = client.call("decompile_function_by_address", {"address": address})
            path = Path(__file__).resolve().parents[1] / "research" / "decompiled" / ("mcp_" + address + ".c")
            path.write_text("\n".join(c.get("text", "") for c in result.get("content", [])))
            print(str(path), path.stat().st_size)
        raise SystemExit(0)
    result = client.request("tools/list", {}) if args.tool == "tools/list" else client.call(args.tool, json.loads(args.arguments))
    output = json.dumps(result, indent=2)
    if args.out:
        Path(args.out).write_text(output)
    print(output)
