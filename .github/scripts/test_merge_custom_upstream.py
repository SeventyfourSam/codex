import contextlib
import io
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import merge_custom_upstream as merge
import sync_custom_release as sync


class MergeUpstreamTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="codex-merge-test-")
        self.addCleanup(self.directory.cleanup)
        previous = Path.cwd()
        os.chdir(self.directory.name)
        self.addCleanup(os.chdir, previous)
        self.git("init", "-b", "custom")
        self.git("config", "user.name", "Merge test")
        self.git("config", "user.email", "merge@example.invalid")
        self.git("config", "commit.gpgsign", "false")
        self.git("config", "core.autocrlf", "false")
        self.git(
            "config",
            f"url.{Path.cwd().as_posix()}.insteadOf",
            "https://github.com/openai/codex.git",
        )
        Path("codex-rs").mkdir()
        self.manifest = Path(merge.MANIFEST)
        self.manifest.write_text(
            '[workspace.package]\nversion = "0.0.0"\nedition = "2024"\n'
            '\n[workspace.dependencies]\nshared = "1"\n',
            encoding="utf-8",
            newline="\n",
        )
        Path("feature.txt").write_text("base\n", encoding="utf-8", newline="\n")
        self.commit("Base")
        self.git("branch", "release")
        self.set_version("0.159.0")
        self.commit("Custom version")
        Path("custom.txt").write_text(
            "keep custom changes\n", encoding="utf-8", newline="\n"
        )
        self.commit("Custom feature")

    def git(self, *args):
        return subprocess.check_output(
            ["git", *args], text=True, encoding="utf-8", stderr=subprocess.STDOUT
        ).strip()

    def commit(self, message):
        self.git("add", ".")
        self.git("commit", "-m", message)

    def set_version(self, version):
        self.manifest.write_text(
            merge.replace_version(self.manifest.read_text(encoding="utf-8"), version),
            encoding="utf-8",
            newline="\n",
        )

    def release(self):
        self.git("switch", "release")
        Path("feature.txt").write_text(
            "upstream feature\n", encoding="utf-8", newline="\n"
        )
        self.commit("Upstream feature")
        self.set_version("0.159.1")
        self.commit("Release version")
        self.git("tag", "rust-v0.159.1")
        self.git("switch", "custom")

    def run_merge(self):
        with (
            patch("sys.argv", ["merge_custom_upstream.py", "0.159.1"]),
            patch.dict(os.environ, {"GITHUB_STEP_SUMMARY": ""}),
            contextlib.redirect_stdout(io.StringIO()),
        ):
            merge.main()

    def test_merge_preserves_custom_and_upstream_changes_and_is_idempotent(self):
        self.release()
        before = self.git("rev-parse", "HEAD")
        self.run_merge()
        self.assertEqual(self.git("rev-parse", "HEAD^1"), before)
        self.assertEqual(
            self.git("rev-parse", "HEAD^2"), self.git("rev-parse", "rust-v0.159.1")
        )
        self.assertEqual(
            Path("custom.txt").read_text(encoding="utf-8"), "keep custom changes\n"
        )
        self.assertEqual(
            Path("feature.txt").read_text(encoding="utf-8"), "upstream feature\n"
        )
        self.assertEqual(
            merge.tomllib.loads(self.manifest.read_text(encoding="utf-8")),
            {
                "workspace": {
                    "package": {"version": "0.159.1", "edition": "2024"},
                    "dependencies": {"shared": "1"},
                }
            },
        )
        self.assertEqual(self.git("status", "--porcelain"), "")
        merged = self.git("rev-parse", "HEAD")
        self.run_merge()
        self.assertEqual(self.git("rev-parse", "HEAD"), merged)

    def test_merge_preserves_upstream_snapshot_padding(self):
        self.git("switch", "release")
        snapshot = Path("screen.snap")
        content = b"menu\n> selected       \n  next           \n\n"
        snapshot.write_bytes(content)
        self.commit("Upstream padded snapshot")
        self.git("switch", "custom")
        self.release()
        self.run_merge()
        self.assertEqual(snapshot.read_bytes(), content)
        self.assertEqual(self.git("status", "--porcelain"), "")

    def test_leftover_conflict_markers_do_not_commit(self):
        self.git("switch", "release")
        Path("broken.txt").write_text(
            "<<<<<<< ours\nleft\n=======\nright\n>>>>>>> theirs\n",
            encoding="utf-8",
            newline="\n",
        )
        self.commit("Upstream leftover markers")
        self.git("switch", "custom")
        self.release()
        before = self.git("rev-parse", "HEAD")
        with self.assertRaises(subprocess.CalledProcessError):
            self.run_merge()
        self.assertEqual(self.git("rev-parse", "HEAD"), before)

    def test_other_file_conflict_does_not_commit(self):
        self.release()
        Path("feature.txt").write_text(
            "custom feature\n", encoding="utf-8", newline="\n"
        )
        self.commit("Conflicting custom feature")
        before = self.git("rev-parse", "HEAD")
        with self.assertRaisesRegex(ValueError, "manual resolution"):
            self.run_merge()
        self.assertEqual(self.git("rev-parse", "HEAD"), before)
        self.assertEqual(
            "feature.txt", self.git("diff", "--name-only", "--diff-filter=U")
        )
        self.assertEqual(
            merge.tomllib.loads(self.manifest.read_text(encoding="utf-8"))["workspace"][
                "package"
            ]["version"],
            "0.159.1",
        )

    def test_dependency_conflict_does_not_commit(self):
        self.manifest.write_text(
            self.manifest.read_text(encoding="utf-8").replace(
                'shared = "1"', 'shared = "2"'
            ),
            encoding="utf-8",
            newline="\n",
        )
        self.commit("Custom dependency")
        self.git("switch", "release")
        self.manifest.write_text(
            self.manifest.read_text(encoding="utf-8").replace(
                'shared = "1"', 'shared = "3"'
            ),
            encoding="utf-8",
            newline="\n",
        )
        self.commit("Upstream dependency")
        self.git("switch", "custom")
        self.release()
        before = self.git("rev-parse", "HEAD")
        with self.assertRaisesRegex(ValueError, "beyond the workspace version"):
            self.run_merge()
        self.assertEqual(self.git("rev-parse", "HEAD"), before)
        self.assertEqual(
            self.git("diff", "--name-only", "--diff-filter=U"), merge.MANIFEST
        )

    def prepare_sync(self, revision=4):
        suffix = "" if revision is None else f".{revision}"
        self.git("tag", f"v0.159.0-custom{suffix}")
        remote = Path.cwd() / ".git" / "origin.git"
        self.git("init", "--bare", str(remote))
        self.git("remote", "add", "origin", str(remote))
        self.git("push", "origin", "custom", "--tags")

    def sync_release(self, version, release=None):
        official = {"tag_name": f"rust-v{version}", "draft": False, "prerelease": False}
        with (
            patch.object(sync, "github_api", side_effect=[official, release]),
            patch.dict(os.environ, {"GITHUB_STEP_SUMMARY": ""}),
        ):
            return sync.sync_release("example/codex")

    def test_sync_upgrades_preserves_revision_and_retries_same_tag(self):
        self.prepare_sync()
        self.release()
        tag = self.sync_release("0.159.1")
        self.assertEqual(tag, "v0.159.1-custom.4")
        head = self.git("rev-parse", "HEAD")
        self.assertEqual(self.git("rev-parse", f"{tag}^{{commit}}"), head)
        self.assertEqual(
            self.git("ls-remote", "origin", "refs/heads/custom"),
            f"{head}\trefs/heads/custom",
        )
        self.assertEqual(
            self.git("ls-remote", "origin", f"refs/tags/{tag}^{{}}"),
            f"{head}\trefs/tags/{tag}^{{}}",
        )
        # Failed or partial builds retry the original tag, even with new commits.
        Path("custom.txt").write_text(
            "unreleased change\n", encoding="utf-8", newline="\n"
        )
        self.commit("Unreleased work")
        self.assertEqual(self.sync_release("0.159.1"), tag)
        self.assertEqual(self.git("rev-parse", f"{tag}^{{commit}}"), head)
        partial = {
            "draft": False,
            "prerelease": False,
            "assets": [{"name": "SHA256SUMS"}],
        }
        self.assertEqual(self.sync_release("0.159.1", partial), tag)

    def test_sync_does_not_publish_new_custom_changes_without_a_new_tag(self):
        self.prepare_sync()
        Path("custom.txt").write_text(
            "unreleased change\n", encoding="utf-8", newline="\n"
        )
        self.commit("Unreleased work")
        before = self.git("rev-parse", "HEAD")
        published = {
            "draft": False,
            "prerelease": False,
            "assets": [
                {"name": name}
                for name in [
                    "codex-0.159.0-custom.4-aarch64-apple-darwin.tar.gz",
                    "codex-0.159.0-custom.4-x86_64-pc-windows-msvc.zip",
                    "install.sh",
                    "install.ps1",
                    "SHA256SUMS",
                ]
            ],
        }
        self.assertEqual(self.sync_release("0.159.0", published), "")
        self.assertEqual(self.git("rev-parse", "HEAD"), before)
        self.assertEqual(self.git("tag", "--list", "v*-custom.*"), "v0.159.0-custom.4")

    def test_sync_carries_new_manual_revision_forward(self):
        self.prepare_sync()
        self.git("tag", "v0.159.0-custom.5")
        self.release()
        self.assertEqual(self.sync_release("0.159.1"), "v0.159.1-custom.5")

    def test_sync_unnumbered_revision_upgrades_and_retries_original_tag(self):
        self.prepare_sync(revision=None)
        self.release()
        tag = self.sync_release("0.159.1")
        self.assertEqual(tag, "v0.159.1-custom")
        head = self.git("rev-parse", "HEAD")
        self.assertEqual(self.git("rev-parse", f"{tag}^{{commit}}"), head)
        self.assertEqual(self.sync_release("0.159.1"), tag)
        self.assertEqual(
            self.git("ls-remote", "origin", f"refs/tags/{tag}^{{}}"),
            f"{head}\trefs/tags/{tag}^{{}}",
        )

    def test_sync_explicit_zero_retries_and_preserves_spelling(self):
        self.prepare_sync(revision=0)
        self.assertEqual(self.sync_release("0.159.0"), "v0.159.0-custom.0")
        self.release()
        self.assertEqual(self.sync_release("0.159.1"), "v0.159.1-custom.0")

    def test_sync_numbered_revision_supersedes_unnumbered(self):
        self.prepare_sync(revision=None)
        self.git("tag", "v0.159.0-custom.1")
        self.release()
        self.assertEqual(self.sync_release("0.159.1"), "v0.159.1-custom.1")

    def test_sync_after_squash_preserves_recorded_revision(self):
        self.prepare_sync()
        base = self.git("merge-base", "custom", "release")
        tree = self.git("rev-parse", "HEAD^{tree}")
        squashed = self.git("commit-tree", tree, "-p", base, "-m", "Squashed custom")
        self.git("switch", "-C", "custom", squashed)
        self.assertEqual(
            self.git("tag", "--merged", "HEAD", "--list", "v*-custom.*"), ""
        )
        Path(".github").mkdir()
        Path(".github/custom-release-baseline").write_text(
            "v0.159.0-custom.4\n", encoding="utf-8", newline="\n"
        )
        self.commit("Record release baseline")
        self.git("push", "--force", "origin", "custom")
        self.release()
        self.assertEqual(self.sync_release("0.159.1"), "v0.159.1-custom.4")

    def test_sync_rejects_prerelease_and_leaves_remote_unchanged(self):
        self.prepare_sync()
        before = self.git("ls-remote", "origin")
        with self.assertRaisesRegex(ValueError, "stable version"):
            self.sync_release("0.160.0-alpha.1")
        self.assertEqual(self.git("ls-remote", "origin"), before)

    def test_sync_conflict_does_not_push_or_tag(self):
        self.prepare_sync()
        self.release()
        Path("feature.txt").write_text(
            "conflicting custom work\n", encoding="utf-8", newline="\n"
        )
        self.commit("Conflicting custom work")
        before = self.git("ls-remote", "origin")
        with self.assertRaisesRegex(ValueError, "manual resolution"):
            self.sync_release("0.159.1")
        self.assertEqual(self.git("ls-remote", "origin"), before)
        self.assertEqual(self.git("tag", "--list", "v0.159.1-custom.*"), "")

    def test_sync_concurrent_push_rejects_both_remote_updates(self):
        self.prepare_sync()
        self.release()
        previous = self.git("rev-parse", "HEAD")
        Path("other.txt").write_text(
            "someone else's work\n", encoding="utf-8", newline="\n"
        )
        self.commit("Concurrent remote change")
        self.git("push", "origin", "custom")
        self.git("switch", "-C", "custom", previous)
        before = self.git("ls-remote", "origin")
        with self.assertRaises(subprocess.CalledProcessError):
            self.sync_release("0.159.1")
        self.assertEqual(self.git("ls-remote", "origin"), before)


if __name__ == "__main__":
    unittest.main()
