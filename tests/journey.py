import json
import os
import pathlib
import secrets
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import zipfile

binary = str(pathlib.Path(sys.argv[1]).resolve())
password = secrets.token_urlsafe(24)


def run(*args, stdin=None, ok=True):
    p = subprocess.run([binary, *map(str, args)], input=stdin, text=True, capture_output=True)
    assert (p.returncode == 0) == ok, p.stderr  # GATE
    return json.loads(p.stdout) if p.returncode == 0 else p.stderr


with tempfile.TemporaryDirectory(prefix="gesa-journey-") as temporary:
    root = pathlib.Path(temporary)
    service = root / "server"
    node = root / "node"
    admin = ("admin", "--root", service)
    for account in ("company-a", "company-b"):
        run(*admin, "account", account, "--label", account, "--password-stdin", stdin=password + "\n")
    run(*admin, "product", "demo", "--label", "Deployment fixture")
    target = "windows-x86_64" if os.name == "nt" else "linux-x86_64"
    artifact = root / "demo.zip"

    def publish(version, entry="readme.txt"):
        with zipfile.ZipFile(artifact, "w") as z:
            z.writestr(entry, version)
        run(*admin, "publish", "demo", "--version", version, "--target", target,
            "--format", "zip", "--artifact", artifact)

    publish("1.0.0")
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    url = f"http://127.0.0.1:{port}"
    process = subprocess.Popen([binary, "server", "--root", str(service), "--listen", f"127.0.0.1:{port}"], stdout=subprocess.DEVNULL)

    def api(method, route, token=None, body=None, expected=200):
        headers = {"Content-Type": "application/json"}
        if token:
            headers["Authorization"] = "Bearer " + token
        req = urllib.request.Request(url + "/" + route, data=json.dumps(body).encode() if body is not None else None, headers=headers, method=method)
        try:
            response = urllib.request.urlopen(req)
        except urllib.error.HTTPError as error:
            response = error
        assert response.status == expected, (route, response.status)  # GATE
        return json.load(response)

    try:
        for attempt in range(100):
            try:
                api("GET", "health")
                break
            except urllib.error.URLError:
                time.sleep(0.05)
        api("GET", "devices", expected=401)
        api("POST", "session/login", body={"account": "company-a", "password": "wrong"}, expected=401)
        run("--home", node, "configure", "--host", url)
        first = run("--home", node, "login", "--credentials-stdin", stdin=json.dumps({"account": "company-a", "password": password}))
        run("--home", node, "login", "--credentials-stdin", stdin=json.dumps({"account": "company-a", "password": password}))
        browser = api("POST", "session/login", body={"account": "company-a", "password": password})["token"]
        other = api("POST", "session/login", body={"account": "company-b", "password": password})["token"]
        assert len(api("GET", "devices", browser)["devices"]) == 1  # GATE
        assert api("GET", "devices", other)["devices"] == []  # GATE
        selection = api("POST", "selections", browser, {"product": "demo", "device": first["device"]["id"]})
        assert selection["status"] == "permission_requested"  # GATE
        run("--home", node, "install", "demo", ok=False)
        run(*admin, "grant", "company-a", "demo", "--device", first["device"]["id"])
        assert run("--home", node, "install", "demo")["status"] == "installed"  # GATE
        assert run("--home", node, "install", "demo")["status"] == "current"  # GATE
        installed_path = next((node / "products").glob("*/installed.json"))
        state = json.loads(installed_path.read_text())
        assert pathlib.Path(state["directory"], "readme.txt").read_text() == "1.0.0"  # GATE
        if os.name == "nt":
            run("--home", node, "auto-update", "demo", "enable")
            run("--home", node, "auto-update", "demo", "disable")
        api("GET", f"artifacts/{state['release']['sha256']}", other, expected=403)
        publish("1.1.0")
        state["auto_update"] = True
        installed_path.write_text(json.dumps(state))
        assert run("--home", node, "sync")["products"][0]["version"] == "1.1.0"  # GATE
        publish("2.0.0", "../escape.txt")
        run("--home", node, "install", "demo", ok=False)
        assert not (root / "escape.txt").exists()  # GATE
        state = json.loads(installed_path.read_text())
        assert state["release"]["version"] == "1.1.0"  # GATE
        run(*admin, "revoke", "company-a", "demo", "--device", first["device"]["id"])
        run("--home", node, "install", "demo", ok=False)
        api("DELETE", "session", browser)
        api("GET", "devices", browser, expected=401)
        print("Enterprise journey passed: enrollment, isolation, selection, grant, installation, update, ZIP escape, revocation, logout")
    finally:
        process.terminate()
        process.wait(timeout=10)
