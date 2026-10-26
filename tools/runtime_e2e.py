#!/usr/bin/env python3
"""Real Redis, Rust service hosts, isolated accounts and CLI lifecycle acceptance."""
import argparse
import contextlib
import hashlib
import json
import os
import signal
from pathlib import Path
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request


def port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def write(path, value):
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    path.write_text(json.dumps(value))
    path.chmod(0o600)
    return path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin", required=True, type=Path)
    parser.add_argument("--redis", required=True, type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    # Redis distributions use an argv[0]-selected multicall executable behind this symlink.
    binary, redis = args.bin.resolve(), args.redis.absolute()
    passed = []
    with tempfile.TemporaryDirectory(prefix="gesac-runtime-") as temp, contextlib.ExitStack() as stack:
        root = Path(temp)
        processes = []

        def spawn(command, **kwargs):
            log = stack.enter_context(open(root / f"process-{len(processes)}.log", "w+"))
            proc = subprocess.Popen(command, stdout=log, stderr=log, start_new_session=True, **kwargs)
            processes.append(proc)
            return proc

        def stop(proc):
            if proc.poll() is None:
                os.killpg(proc.pid, signal.SIGTERM)
                try:
                    proc.wait(10)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()

        stack.callback(lambda: [stop(p) for p in reversed(processes)])

        def run(*command, home=None, stdin=None, ok=True):
            env = dict(os.environ)
            if home:
                env["GESAC_HOME"] = str(home)
            proc = subprocess.run([str(binary), *map(str, command)], input=json.dumps(stdin) if stdin is not None else None,
                                  text=True, capture_output=True, env=env, timeout=120)
            if ok:
                if proc.returncode:
                    for log in root.glob("process-*.log"):
                        print(log.name, log.read_text()[-3000:], file=sys.stderr)
                assert proc.returncode == 0, (command, proc.stderr)
                return json.loads(proc.stdout)["result"]
            assert proc.returncode != 0, command
            return proc

        def wait(url):
            for _ in range(150):
                try:
                    with urllib.request.urlopen(url, timeout=.5):
                        return
                except (OSError, urllib.error.URLError):
                    time.sleep(.1)
            raise AssertionError("service did not become ready")

        redis_port = port()
        redis_args = [str(redis), "--bind", "127.0.0.1", "--port", str(redis_port), "--save", "", "--appendonly", "no"]
        redis_proc = spawn(redis_args)
        for _ in range(100):
            try:
                with socket.create_connection(("127.0.0.1", redis_port), timeout=.2):
                    break
            except OSError:
                assert redis_proc.poll() is None, "Redis server exited during startup"
                time.sleep(.05)
        else:
            raise AssertionError("Redis server did not become ready")
        redis_config = write(root / "redis.json", {"url": f"redis://127.0.0.1:{redis_port}/"})
        hosts = []
        for index in range(2):
            server_root, server_port = root / f"host{index}", port()
            creds = {"account": "owner", "password": f"private-password-{index}", "passphrase": f"private-passphrase-{index}"}
            identity = run("server", "init", "--root", server_root, "--listen", f"127.0.0.1:{server_port}", "--redis-config", redis_config, stdin=creds)
            run("server", "plan", "--root", server_root, "--id", "tools", "--file", write(root / f"plan{index}.json", {"name": "Tools"}))
            run("server", "grant", "--root", server_root, "--account", "owner", "--plan", "tools")
            run("server", "plan", "--root", server_root, "--id", "gesac", "--file", root / f"plan{index}.json")
            run("server", "grant", "--root", server_root, "--account", "owner", "--plan", "gesac")
            proc = spawn([str(binary), "server", "serve", "--root", str(server_root)])
            wait(f"http://127.0.0.1:{server_port}/health")
            profile = {"id": f"host{index}", "host_id": identity["host_id"], "connection": {
                "schema": 1, "center_url": f"http://127.0.0.1:{server_port}", "account": "owner", "release_public_key": identity["public_key"],
                "channel": "stable", "allow_http_loopback": True, "credential_store": "file"}}
            hosts.append((server_root, creds, profile, proc))

        home = root / "node"
        run("node", "init", home=home)
        for i, (_, creds, profile, _) in enumerate(hosts):
            run("node", "profile-add", "--file", write(root / f"profile{i}.json", profile), home=home, stdin={k: creds[k] for k in ("password", "passphrase")})
            run("node", "subscribe", "--profile", profile["id"], "--plan", "tools", home=home)
        assert len(run("node", "profiles", home=home)) == 2
        passed.append("two hosts and independent credentials")

        def publish(index, sequence, content):
            server_root, _, profile, _ = hosts[index]
            directory = root / f"files-{index}-{sequence}"
            directory.mkdir()
            (directory / "hello.txt").write_text(content)
            meta = {"schema": 1, "host_id": profile["host_id"], "plan": "tools", "product": "content", "version": f"1.0.{sequence}",
                    "target": "any", "channel": "stable", "sequence": sequence, "expires_at": int(time.time()) + 3600, "files": []}
            run("server", "publish", "--root", server_root, "--file", write(root / f"release-{index}-{sequence}.json", meta), "--directory", directory)

        def instance(id="content", profile="host0", adapter=None):
            value = {"id": id, "profile": profile, "plan": "tools", "product": "content", "target": "any", "channel": "stable"}
            if adapter:
                value["adapter"] = adapter
            return value

        publish(0, 1, "first")
        run("node", "register", "--file", write(root / "instance.json", instance()), home=home)
        op = run("node", "stage", "--instance", "content", home=home)
        assert run("node", "stage", "--instance", "content", home=home)["id"] == op["id"]
        run("node", "prepare", "--operation", op["id"], home=home)
        run("node", "apply", "--operation", op["id"], home=home)
        assert run("node", "verify", "--instance", "content", "--version", "1.0.1", home=home)["verified"]
        run("node", "verify", "--instance", "content", "--version", "9.0.0", home=home, ok=False)
        run("node", "export", "--operation", op["id"], "--destination", root / "offline", home=home)
        passed.append("signed online lifecycle, duplicate admission and version gate")

        # Another device uses the same account and remains a separate target.
        second = root / "second-node"
        creds = {k: hosts[0][1][k] for k in ("password", "passphrase")}
        run("node", "profile-add", "--file", root / "profile0.json", home=second, stdin=creds)
        run("node", "subscribe", "--profile", "host0", "--plan", "tools", home=second)
        run("node", "register", "--file", root / "instance.json", home=second)
        second_op = run("node", "stage", "--instance", "content", home=second)
        assert second_op["id"] != op["id"]
        passed.append("same-account multi-device update")

        stop(redis_proc)
        stop(hosts[0][3])
        offline = run("node", "stage", "--instance", "content", "--offline", root / "offline", home=home)
        assert offline["id"] == op["id"]
        # A fresh offline instance actually installs with neither Redis nor host available.
        run("node", "register", "--file", write(root / "offline-instance.json", instance("offline")), home=second)
        offline = run("node", "stage", "--instance", "offline", "--offline", root / "offline", home=second)
        run("node", "prepare", "--operation", offline["id"], home=second)
        run("node", "apply", "--operation", offline["id"], home=second)
        assert run("node", "verify", "--instance", "offline", home=second)["verified"]
        passed.append("offline installation without host or Redis")

        # Restore Redis with no state; durable service admission still blocks a second target operation.
        redis_proc = spawn(redis_args)
        proc = spawn([str(binary), "server", "serve", "--root", str(hosts[0][0])])
        wait(hosts[0][2]["connection"]["center_url"] + "/health")
        node_db = sqlite3.connect(second / "node.sqlite")
        profile_home = second / "profiles" / "host0"
        session_file = next(profile_home.glob("session-*.secret"))
        token = json.loads(session_file.read_text())["access_token"]
        url = hosts[0][2]["connection"]["center_url"]
        req = urllib.request.Request(url + "/v1/operations", data=json.dumps({"id": "different-operation", "instance": "content", "release": second_op["release_id"]}).encode(),
                                     headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"})
        try:
            urllib.request.urlopen(req)
            raise AssertionError("unresolved target admitted duplicate")
        except urllib.error.HTTPError as exc:
            assert exc.code == 409
        node_db.close()
        passed.append("server/Redis restart preserves unresolved target ownership")

        publish(0, 2, "second")
        next_op = run("node", "stage", "--instance", "content", home=home)
        original = Path(next_op["directory"]) / "hello.txt"
        original.write_text("tampered")
        run("node", "prepare", "--operation", next_op["id"], home=home, ok=False)
        original.write_text("second")
        run("node", "prepare", "--operation", next_op["id"], home=home)
        run("node", "apply", "--operation", next_op["id"], home=home)
        passed.append("tampered payload rejected before activation")

        # An uncertain local adapter is invoked once, then reconciled without replay.
        adapter = root / "adapter.py"
        count = root / "adapter-count"
        adapter.write_text("import sys,json\nfrom pathlib import Path\nx=json.load(sys.stdin)\np=Path(" + repr(str(count)) + ")\n"
                           "if x['action']=='apply':\n p.write_text('executed')\n sys.exit(1)\n"
                           "print(json.dumps({'status':'prepared' if x['action']=='prepare' else 'applied'}))\n")
        run("node", "register", "--file", write(root / "adapter-instance.json", instance("uncertain", adapter={"file": sys.executable, "args": [str(adapter)], "timeout_seconds": 5})), home=home)
        uncertain = run("node", "stage", "--instance", "uncertain", home=home)
        run("node", "prepare", "--operation", uncertain["id"], home=home)
        assert run("node", "apply", "--operation", uncertain["id"], home=home)["status"] == "unknown"
        run("node", "apply", "--operation", uncertain["id"], home=home, ok=False)
        assert count.read_text() == "executed"
        assert run("node", "reconcile", "--operation", uncertain["id"], home=home)["status"] == "applied"
        passed.append("unknown execution reconciles without replay")

        # The real service host also supplies the independent launcher's self-update protocol.
        self_candidate = root / "self-candidate"
        self_candidate.write_bytes(binary.read_bytes() + b"\0GESAC isolated signed candidate\n")
        self_candidate.chmod(0o700)
        artifact_hash = hashlib.sha256(self_candidate.read_bytes()).hexdigest()
        self_release = {"schema": 1, "sequence": 1, "version": "0.2.0", "channel": "stable", "target": "linux-x86_64",
                        "artifact": "/artifacts/" + artifact_hash, "sha256": artifact_hash, "size": self_candidate.stat().st_size,
                        "expires_at": int(time.time()) + 3600, "protocol": 1, "contract_epoch": 1, "supports_contracts": [1],
                        "iteration": {"candidate_id": "runtime-self-update", "source_revision": "a" * 40, "artifact_sha256": artifact_hash,
                                      "change_summary": "isolated self-update fixture", "risk": "low", "tests": [{"name": "runtime", "passed": True, "evidence_sha256": "b" * 64}],
                                      "approval": {"decision": "approved", "actor": "test", "policy": "automatic-low-risk"},
                                      "rollback": "retained bootstrap", "migration": {"kind": "none", "reversible": True}, "protected_core_changed": False}}
        run("server", "publish-self", "--root", hosts[0][0], "--file", write(root / "self-release.json", self_release), "--binary", self_candidate)
        run("node", "self-source", "--profile", "host0", home=home)
        assert run("update", home=home)["updated"]
        assert run("verify", home=home)["verified"]
        assert run("update", home=home)["reason"] == "already_current"
        passed.append("service-host self-update, offline verification and duplicate suppression")

        stop(proc)
        supervisor = spawn([str(binary), "server", "supervise", "--root", str(hosts[0][0])], env={**os.environ, "GESAC_HOME": str(home)})
        wait(url + "/health")
        children = subprocess.check_output(["ps", "--ppid", str(supervisor.pid), "-o", "pid="], text=True).strip()
        assert children
        run("rollback", home=home)
        for _ in range(80):
            time.sleep(.1)
            current_children = subprocess.run(["ps", "--ppid", str(supervisor.pid), "-o", "pid="], text=True, capture_output=True).stdout.strip()
            if current_children and current_children != children:
                break
        assert current_children != children
        wait(url + "/health")
        assert len(run("node", "plans", "--profile", "host0", home=home)["plans"]) == 2
        passed.append("service supervisor adopts self-update selection and preserves account database")
        stop(supervisor)

        run("uninstall", home=home)
        for _ in range(100):
            if not home.exists():
                break
            time.sleep(.1)
        assert not home.exists()
        assert count.read_text() == "executed"
        assert (hosts[0][0] / "server.sqlite").is_file()
        passed.append("independent self-uninstall clears configuration and retains external data")

    report = {"suite": "gesac-runtime-e2e", "passed": passed, "count": len(passed)}
    if args.report:
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))


if __name__ == "__main__":
    main()
