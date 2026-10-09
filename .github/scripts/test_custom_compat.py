import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
import subprocess

import run_custom_compat_tests as compat


class CompatRunnerTests(unittest.TestCase):
    def test_failure_restores_version_and_lockfile(self):
        with tempfile.TemporaryDirectory(prefix="codex-compat-test-") as directory:
            previous = Path.cwd()
            os.chdir(directory)
            try:
                Path("codex-rs").mkdir()
                manifest = Path("codex-rs/Cargo.toml")
                original = b'[workspace.package]\nversion = "0.161.0"\n'
                manifest.write_bytes(original)
                lock = Path("codex-rs/Cargo.lock")
                lock.write_bytes(b"merged lock\n")

                def run(command, **kwargs):
                    self.assertEqual(command, ["just", "test", "-p", "codex-tui"])
                    self.assertIn('version = "0.0.0"', manifest.read_text())
                    self.assertEqual(
                        kwargs["env"]["CODEX_CUSTOM_USAGE_POLICY"], "upstream"
                    )
                    lock.write_bytes(b"Cargo normalized versions\n")
                    return subprocess.CompletedProcess(command, 1)

                with (
                    patch.object(compat.subprocess, "run", side_effect=run),
                    patch("sys.argv", ["runner", "-p", "codex-tui"]),
                ):
                    with self.assertRaises(SystemExit) as error:
                        compat.main()
                self.assertEqual(error.exception.code, 1)
                self.assertEqual(manifest.read_bytes(), original)
                self.assertEqual(lock.read_bytes(), b"merged lock\n")
            finally:
                os.chdir(previous)
