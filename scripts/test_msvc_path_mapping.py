"""Exercise early refusal of unsupported or ineffective native path mapping."""

import importlib.util
from pathlib import Path, PureWindowsPath
import subprocess
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("dev", Path(__file__).with_name("dev.py"))
DEV = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DEV)


class MsvcMappingTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        patch = mock.patch.object(DEV, "ROOT", Path(temporary.name))
        patch.start()
        self.addCleanup(patch.stop)

    def test_accepts_only_the_mapped_marker_and_requests_preprocessing(self):
        output = 'const char *echocord_probe_source = "/echocord/.tools/probe.c";\n'
        with mock.patch.object(DEV, "run", return_value=output) as compiler:
            with mock.patch("builtins.print"):
                DEV.verify_msvc_path_mapping({"_CL_": "mapped flags"})
        self.assertIn("/EP", compiler.call_args.args)
        self.assertIn("/FC", compiler.call_args.args)
        self.assertEqual(compiler.call_args.kwargs["env"], {"_CL_": "mapped flags"})

    def test_ignored_mapping_is_rejected_before_build(self):
        output = r'const char *echocord_probe_source = "C:\\checkout\\probe.c";'
        with mock.patch.object(DEV, "run", return_value=output):
            with self.assertRaisesRegex(SystemExit, "did not remap"):
                DEV.verify_msvc_path_mapping({})

    def test_failed_compiler_is_rejected(self):
        with mock.patch.object(
            DEV, "run", side_effect=subprocess.CalledProcessError(1, "cl")
        ):
            with self.assertRaisesRegex(SystemExit, "probe failed"):
                DEV.verify_msvc_path_mapping({})

    def test_options_preserve_spaces_user_flags_and_parent_environment(self):
        original = {"_CL_": "/DUSER_BUILD", "UNRELATED": "retained"}
        mapped = DEV.msvc_path_environment(
            original,
            [(PureWindowsPath(r"C:\workspace with spaces\echocord"), "/echocord")],
        )
        self.assertEqual(original, {"_CL_": "/DUSER_BUILD", "UNRELATED": "retained"})
        self.assertEqual(mapped["UNRELATED"], "retained")
        self.assertEqual(
            mapped["_CL_"],
            '/DUSER_BUILD "/pathmap:C:\\workspace with spaces\\echocord=/echocord"',
        )


if __name__ == "__main__":
    unittest.main()
