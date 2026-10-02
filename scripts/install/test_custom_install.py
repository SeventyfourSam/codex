"""Exercise the fork installer without network access or touching the user's install."""

import hashlib
import json
import os
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("custom-install.sh").resolve()
REPO = "https://github.com/SeventyfourSam/codex"
VERSION = "0.159.0-custom.2"


class CustomInstallTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.bin = self.root / "tools"
        self.bin.mkdir()
        self.home = self.root / "home with spaces"
        self.home.mkdir()
        self.env = dict(
            os.environ,
            HOME=str(self.home),
            CODEX_HOME=str(self.home / ".codex"),
            CODEX_INSTALL_DIR=str(self.home / "bin"),
            SHELL="/bin/zsh",
            ZDOTDIR=str(self.home),
            PATH=f"{self.bin}:{os.environ['PATH']}",
            FIXTURE=str(self.root),
        )
        for key in list(self.env):
            if key.startswith("CODEX_INSTALL_") and key != "CODEX_INSTALL_DIR":
                del self.env[key]
        self.executable(
            self.bin / "uname",
            '#!/bin/sh\ncase "$1" in -s) echo Darwin;; -m) echo arm64;; esac\n',
        )
        # Every unexpected URL fails: this test can never reach a real network.
        self.executable(
            self.bin / "curl",
            f"""#!{sys.executable}
import json, os, pathlib, shutil, sys
root = pathlib.Path(os.environ['FIXTURE'])
args = sys.argv[1:]
url = next(a for a in args if a.startswith('https://'))
with (root / 'requests').open('a') as f: f.write(url + '\\n')
if '-I' in args:
    print('{REPO}/releases/tag/v{VERSION}', end='')
else:
    mapping = json.loads((root / 'mapping').read_text())
    if url not in mapping: sys.exit(91)
    shutil.copyfile(mapping[url], args[args.index('-o') + 1])
""",
        )
        self.make_release(VERSION)

    def executable(self, path, content):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        path.chmod(0o755)

    def make_release(self, version):
        package = self.root / version
        self.executable(
            package / "bin/codex",
            "#!/bin/sh\n"
            'if [ "${CUSTOM_TEST_REQUIRE_RELEASE_PATH:-0}" = 1 ]; then\n'
            "  binary=$0\n"
            '  if [ -L "$binary" ]; then binary=$(readlink "$binary"); fi\n'
            '  directory=$(cd "$(dirname "$binary")" && pwd -P)\n'
            '  case "$directory" in */release/*) ;; *) exit 137;; esac\n'
            "fi\n"
            'if [ "$1" = --initialize-custom-config ]; then\n'
            '  printf "initialized\\n" >> "$FIXTURE/config-init"\n'
            '  [ "${CUSTOM_TEST_CONFIG_FAIL:-0}" != 1 ]; exit $?\n'
            "fi\n"
            f'case "$1" in --version) echo "codex-cli {version.split("-custom")[0]}";; --custom) echo "codex-cli {version}";; app-server) exit 0;; *) exit 2;; esac\n',
        )
        for file in [
            "bin/codex-code-mode-host",
            "codex-path/rg",
            "codex-resources/zsh/bin/zsh",
        ]:
            self.executable(package / file, "#!/bin/sh\nexit 0\n")
        (package / "codex-package.json").write_text(json.dumps({"version": "0.159.0"}))
        asset = f"codex-{version}-aarch64-apple-darwin.tar.gz"
        archive = self.root / asset
        with tarfile.open(archive, "w:gz") as output:
            for path in package.iterdir():
                output.add(path, arcname=path.name)
        sums = self.root / "SHA256SUMS"
        sums.write_text(
            f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {asset}\n"
        )
        (self.root / "mapping").write_text(
            json.dumps(
                {
                    f"{REPO}/releases/download/v{version}/{asset}": str(archive),
                    f"{REPO}/releases/download/v{version}/SHA256SUMS": str(sums),
                }
            )
        )
        return archive

    def install(self, **env):
        return subprocess.run(
            ["sh", str(SCRIPT)],
            env=dict(self.env, CODEX_RELEASE="latest", **env),
            capture_output=True,
            check=False,
            text=True,
            timeout=30,
        )

    def test_install_reuse_and_fixed_version_keep_upstream_version(self):
        for _ in range(2):
            result = self.install()
            self.assertEqual(result.returncode, 0, result.stderr)
        requests = (self.root / "requests").read_text().splitlines()
        self.assertEqual(
            len(requests), 4
        )  # latest twice; checksum and archive only once
        binary = self.home / "bin/codex"
        self.assertEqual(
            subprocess.check_output([binary, "--version"], text=True),
            "codex-cli 0.159.0\n",
        )
        self.assertEqual(
            subprocess.check_output([binary, "--custom"], text=True),
            f"codex-cli {VERSION}\n",
        )
        self.assertEqual((self.home / ".zshrc").read_text().count("/env"), 1)
        self.assertEqual((self.root / "config-init").read_text(), "initialized\n" * 2)
        result = subprocess.run(
            ["sh", str(SCRIPT), "--release", VERSION],
            env=self.env,
            capture_output=True,
            check=False,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(
            (self.home / ".codex/packages/standalone/auto-update-version").exists()
        )

    def test_install_unnumbered_revision_zero(self):
        version = "0.159.0-custom"
        self.make_release(version)
        result = subprocess.run(
            ["sh", str(SCRIPT), "--release", version],
            env=self.env,
            capture_output=True,
            check=False,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        binary = self.home / "bin/codex"
        self.assertEqual(
            subprocess.check_output([binary, "--custom"], text=True),
            f"codex-cli {version}\n",
        )
        self.assertEqual(
            subprocess.check_output([binary, "--version"], text=True),
            "codex-cli 0.159.0\n",
        )

    def test_bad_checksum_keeps_previous_selection(self):
        root = self.home / ".codex/packages/standalone"
        old = root / "releases/old"
        old.mkdir(parents=True)
        (root / "current").symlink_to(old)
        (self.root / "SHA256SUMS").write_text(
            f"{'0' * 64}  codex-{VERSION}-aarch64-apple-darwin.tar.gz\n"
        )
        result = self.install()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SHA-256 mismatch", result.stderr)
        self.assertEqual((root / "current").resolve(), old)

    def test_configuration_failure_keeps_previous_selection(self):
        root = self.home / ".codex/packages/standalone"
        old = root / "releases/old"
        old.mkdir(parents=True)
        (root / "current").symlink_to(old)
        result = self.install(CUSTOM_TEST_CONFIG_FAIL="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((self.root / "config-init").read_text(), "initialized\n")
        self.assertEqual((root / "current").resolve(), old)

    def test_release_path_is_used_during_validation_and_after_install(self):
        result = self.install(CUSTOM_TEST_REQUIRE_RELEASE_PATH="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        root = self.home / ".codex/packages/standalone"
        name = f"{VERSION}-aarch64-apple-darwin"
        self.assertEqual((root / "current").resolve(), root / "releases/release" / name)
        self.assertEqual((root / "auto-update-version").read_text(), name)
        self.assertEqual(list((root / "releases/release").glob(".custom-stage.*")), [])

    def test_daemon_install_and_guard_preserve_visible_cli(self):
        result = self.install(CODEX_INSTALL_DAEMON_ONLY="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.home / "bin").exists())
        root = self.home / ".codex/packages/app-server-daemon"
        self.assertEqual(
            (root / "current").resolve(),
            root / "releases" / f"{VERSION}-aarch64-apple-darwin",
        )
        result = self.install(
            CODEX_INSTALL_DAEMON_ONLY="1",
            CODEX_INSTALL_IF_LATEST="1",
            CODEX_UPDATE_FROM_RELEASE="different",
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(len((self.root / "requests").read_text().splitlines()), 4)

    def test_wrong_binary_version_never_becomes_current(self):
        package = self.root / VERSION
        self.executable(package / "bin/codex", '#!/bin/sh\necho "codex-cli 0.159.0"\n')
        asset = self.root / f"codex-{VERSION}-aarch64-apple-darwin.tar.gz"
        with tarfile.open(asset, "w:gz") as output:
            for path in package.iterdir():
                output.add(path, arcname=path.name)
        (self.root / "SHA256SUMS").write_text(
            f"{hashlib.sha256(asset.read_bytes()).hexdigest()}  {asset.name}\n"
        )
        result = self.install()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.home / ".codex/packages/standalone/current").exists())


if __name__ == "__main__":
    unittest.main()
