"""Exercise the Unix CLI/TUI updater command without network access."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class CustomUpdateTests(unittest.TestCase):
    def test_download_execution_and_failures(self):
        source = (
            Path(__file__).resolve().parents[2] / "codex-rs/tui/src/custom_updates.rs"
        )
        (command,) = [
            json.loads(line.strip().removesuffix(","))
            for line in source.read_text().splitlines()
            if "curl -fsSL" in line
        ]
        for download_status, script, expected_status in [
            (0, "echo installer-ran", 0),
            (22, "echo must-not-run", 22),
            (0, "echo installer-ran; exit 17", 17),
        ]:
            with self.subTest(download=download_status, script=script):
                with tempfile.TemporaryDirectory(prefix="custom update ") as directory:
                    root = Path(directory)
                    curl = root / "curl"
                    curl.write_text(
                        "#!/bin/sh\n"
                        'while [ "$1" != -o ]; do shift; done\n'
                        'printf "%s" "$FIXTURE_SCRIPT" > "$2"\n'
                        'exit "$FIXTURE_STATUS"\n'
                    )
                    curl.chmod(0o755)
                    result = subprocess.run(
                        ["sh", "-c", command],
                        env=dict(
                            os.environ,
                            PATH=f"{root}:{os.environ['PATH']}",
                            TMPDIR=str(root),
                            FIXTURE_SCRIPT=script,
                            FIXTURE_STATUS=str(download_status),
                        ),
                        capture_output=True,
                        text=True,
                        timeout=10,
                    )
                    self.assertEqual(
                        (result.returncode, result.stdout),
                        (expected_status, "" if download_status else "installer-ran\n"),
                        result.stderr,
                    )
                    self.assertEqual(list(root.iterdir()), [curl])


if __name__ == "__main__":
    unittest.main()
