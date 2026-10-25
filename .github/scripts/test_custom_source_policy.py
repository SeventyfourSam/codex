import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import custom_source_policy as policy


class RepositorySourcePolicyTests(unittest.TestCase):
    def test_current_sources_preserve_recorded_upstream_contracts(self):
        root = Path(__file__).resolve().parents[2]
        previous = Path.cwd()
        try:
            os.chdir(root)
            baseline = json.loads(
                Path(".github/custom-upstream-base.json").read_text(encoding="utf-8")
            )
            policy.verify_source_contracts(baseline["commit"])
        finally:
            os.chdir(previous)


class SourcePolicyTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory(prefix="codex-source-policy-")
        self.addCleanup(directory.cleanup)
        previous = Path.cwd()
        os.chdir(directory.name)
        self.addCleanup(os.chdir, previous)
        self.files = {
            path: f"pub struct {name} {{\n    pub existing: bool,\n}}\n"
            for path, name in policy.CONTRACTS
        }
        self.action = "codex-rs/tui/src/update_action.rs"
        self.files[self.action] = "pub enum UpdateAction {\n    Npm,\n    Pnpm,\n}\n"
        self.test_file = "codex-rs/tui/src/history_cell/tests.rs"
        self.files[self.test_file] = "#[tokio::test]\nasync fn upstream_test() {}\n"
        for path, source in self.files.items():
            target = Path(path)
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(source, encoding="utf-8")
        self.mock = patch.object(
            policy, "upstream_file", side_effect=lambda _, path: self.files.get(path)
        )
        self.mock.start()
        self.addCleanup(self.mock.stop)
        self.diff = patch.object(
            policy.subprocess, "check_output", return_value=self.test_file + "\n"
        )
        self.diff.start()
        self.addCleanup(self.diff.stop)

    def test_stock_contracts_are_allowed(self):
        policy.verify_source_contracts("upstream")

    def test_added_field_is_rejected_even_if_json_has_a_default(self):
        path, _ = policy.CONTRACTS[0]
        Path(path).write_text(
            self.files[path].replace(
                "\n}", "\n    #[serde(default)]\n    pub refresh: bool,\n}"
            ),
            encoding="utf-8",
        )
        with self.assertRaisesRegex(
            ValueError, "keep upstream ModelListParams unchanged"
        ):
            policy.verify_source_contracts("upstream")

    def test_deleted_upstream_variant_is_rejected(self):
        Path(self.action).write_text(
            "pub enum UpdateAction {\n    Npm,\n}\n", encoding="utf-8"
        )
        with self.assertRaisesRegex(ValueError, "keep upstream UpdateAction unchanged"):
            policy.verify_source_contracts("upstream")

    def test_deleted_or_renamed_upstream_test_is_rejected(self):
        Path(self.test_file).write_text(
            "#[test]\nfn renamed_test() {}\n", encoding="utf-8"
        )
        with self.assertRaisesRegex(ValueError, "upstream_test"):
            policy.verify_source_contracts("upstream")

    def test_comments_do_not_change_a_contract(self):
        path, _ = policy.CONTRACTS[0]
        Path(path).write_text(
            self.files[path].replace("    pub", "    // Explanation\n    pub"),
            encoding="utf-8",
        )
        policy.verify_source_contracts("upstream")

    def test_added_update_variant_is_rejected(self):
        Path(self.action).write_text(
            "pub enum UpdateAction {\n    Npm,\n    Pnpm,\n    Custom(OwnType),\n}\n",
            encoding="utf-8",
        )
        with self.assertRaisesRegex(ValueError, "keep upstream UpdateAction unchanged"):
            policy.verify_source_contracts("upstream")

    def test_blank_line_only_change_is_rejected(self):
        original = 'mod first;\n#[path = "second.rs"]\nmod second;\n'
        self.files[self.test_file] = original
        Path(self.test_file).write_text(
            original.replace("mod first;\n", "mod first;\n\n"), encoding="utf-8"
        )
        with self.assertRaisesRegex(ValueError, "blank-line-only changes"):
            policy.verify_source_contracts("upstream")

    def test_new_custom_code_can_have_blank_lines(self):
        Path(self.test_file).write_text(
            self.files[self.test_file] + "\nfn custom_helper() {}\n",
            encoding="utf-8",
        )
        policy.verify_source_contracts("upstream")

    def test_blank_line_in_a_string_is_not_treated_as_module_formatting(self):
        original = '#[test]\nfn upstream_test() {\n    let text = r#"a\nb"#;\n}\n'
        self.files[self.test_file] = original
        Path(self.test_file).write_text(
            original.replace("a\nb", "a\n\nb"), encoding="utf-8"
        )
        policy.verify_source_contracts("upstream")
