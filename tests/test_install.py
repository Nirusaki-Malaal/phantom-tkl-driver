import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("installer", ROOT / "tools/install.py")
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


class InstallerTests(unittest.TestCase):
    def test_absolute_launcher_with_spaces(self):
        entry = installer.desktop_entry(Path("/home/example user/.local/bin/phantom-studio"))
        self.assertIn('Exec="/home/example user/.local/bin/phantom-studio"', entry)
        self.assertIn("StartupWMClass=io.github.phantom.Studio", entry)

    def test_launcher_rejects_newlines_and_escapes_field_codes(self):
        with self.assertRaises(ValueError):
            installer.desktop_entry(Path("/tmp/bad\npath"))
        self.assertIn("%%", installer.desktop_entry(Path("/tmp/100%/phantom-studio")))

    def test_rule_update_is_idempotent(self):
        original = 'include "other.kdl"\n'
        rule = 'window-rule { match app-id="test"; open-floating true; }'
        first = installer.replace_managed_rule(original, rule)
        self.assertEqual(first, installer.replace_managed_rule(first, rule))
        self.assertTrue(first.startswith(original))
        self.assertEqual(first.count(installer.START), 1)

    def test_migrates_old_nested_blur_rule(self):
        original = 'include "other.kdl"\n' + installer.OLD + '\nwindow-rule {\n background-effect { blur true; }\n}\n// unrelated\n'
        new = installer.replace_managed_rule(original, 'window-rule { open-floating true; }')
        self.assertNotIn(installer.OLD, new)
        self.assertIn('// unrelated', new)
        self.assertTrue(new.startswith('include "other.kdl"'))

    def test_desktop_repair_preserves_legacy_shortcut_as_alias(self):
        with tempfile.TemporaryDirectory(prefix="phantom install test ") as tmp:
            base = Path(tmp)
            prefix, data, config = base / "prefix", base / "share", base / "config"
            old = data / "applications/phantom-tkl.desktop"
            old.parent.mkdir(parents=True)
            old.write_text("[Desktop Entry]\nName=Phantom old\nExec=/missing/run.sh\n")
            installer.install_desktop(prefix, data, config, configure=False)
            self.assertIn("NoDisplay=true", old.read_text())
            self.assertIn(str(prefix), old.read_text())
            self.assertTrue(list((config / "phantom-studio/backups").glob("*.desktop")))
            entry = data / "applications/io.github.phantom.Studio.desktop"
            self.assertTrue(entry.is_file())


if __name__ == "__main__":
    unittest.main()
