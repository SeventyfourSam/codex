import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


class ReleaseDispatchTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="codex-dispatch-test-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.summary = self.root / "summary.txt"
        self.dispatched = self.root / "dispatched.txt"
        bash = shutil.which("bash")
        if bash is None and os.name == "nt":
            bash = str(Path(os.environ["ProgramFiles"]) / "Git/bin/bash.exe")
        self.bash = bash
        self.assertIsNotNone(self.bash, "Bash is required to test release dispatch")
        workflow = Path(__file__).resolve().parents[1] / "workflows/custom-release.yml"
        step = workflow.read_text(encoding="utf-8").split(
            "      - name: Start release on custom to share caches between tags\n", 1
        )[1]
        block = step.split("        run: |\n", 1)[1]
        lines = []
        for line in block.splitlines():
            if line and not line.startswith("          "):
                break
            lines.append(line[10:])
        self.script = "\n".join(lines)

    def run_dispatch(
        self, tag_type, message, release_tag="v0.160.1-custom.3", ref_status="0"
    ):
        mock = r"""
gh() {
  if [[ "$1" == api && "$2" == */git/ref/tags/* ]]; then
    if [[ "$REF_STATUS" != 0 ]]; then return "$REF_STATUS"; fi
    printf '%s\t%s\n' "$TAG_TYPE" 'tag-sha'
  elif [[ "$1" == api && "$2" == */git/tags/* ]]; then
    printf '%s\n' "$TAG_MESSAGE"
  elif [[ "$1" == workflow && "$2" == run ]]; then
    printf '%s\n' "$@" >> "$DISPATCH_LOG"
  else
    return 2
  fi
}
"""
        return subprocess.run(
            [self.bash, "-c", mock + self.script],
            env={
                **os.environ,
                "TAG_TYPE": tag_type,
                "TAG_MESSAGE": message,
                "REF_STATUS": ref_status,
                "RELEASE_TAG": release_tag,
                "GITHUB_REPOSITORY": "example/codex",
                "GITHUB_STEP_SUMMARY": self.summary.as_posix(),
                "DISPATCH_LOG": self.dispatched.as_posix(),
            },
            capture_output=True,
            text=True,
        )

    def test_upstream_sync_tag_does_not_start_a_second_build(self):
        result = self.run_dispatch(
            "tag", "Merge official 0.160.1; retain custom revision 3"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.dispatched.exists())
        self.assertIn("skipping duplicate dispatch", self.summary.read_text())

    def test_manual_annotated_tag_dispatches_on_custom(self):
        result = self.run_dispatch("tag", "Publish custom changes")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            self.dispatched.read_text().splitlines(),
            [
                "workflow",
                "run",
                "custom-release.yml",
                "--repo",
                "example/codex",
                "--ref",
                "custom",
                "--field",
                "release_tag=v0.160.1-custom.3",
            ],
        )

    def test_manual_lightweight_tag_still_dispatches(self):
        result = self.run_dispatch("commit", "", release_tag="v0.160.1-custom")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("release_tag=v0.160.1-custom", self.dispatched.read_text())

    def test_invalid_tag_stops_before_dispatch(self):
        result = self.run_dispatch("tag", "", release_tag="installers-custom.3")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.dispatched.exists())

    def test_failed_tag_lookup_does_not_dispatch(self):
        result = self.run_dispatch("tag", "", ref_status="9")
        self.assertEqual(result.returncode, 9)
        self.assertFalse(self.dispatched.exists())


if __name__ == "__main__":
    unittest.main()
