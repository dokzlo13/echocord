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

    def result(self, args, *, trim=False, **_):
        if len(args) > 1:
            return subprocess.CompletedProcess(args, 0, "probe.c", "")
        relative = Path(args[0]).with_suffix(".c").relative_to(DEV.ROOT).as_posix()
        mapped = relative if trim else "echocord/" + relative
        return subprocess.CompletedProcess(args, 0, mapped + "\n", "")

    def test_validates_the_compiled_program_output_with_full_paths_enabled(self):
        with mock.patch.object(
            DEV.subprocess, "run", side_effect=self.result
        ) as compiler:
            with mock.patch("builtins.print"):
                DEV.verify_msvc_path_mapping({"_CL_": "mapped flags"})
        calls = compiler.call_args_list
        self.assertEqual(len(calls), 2)
        self.assertIn("/FC", calls[0].args[0])
        self.assertNotIn("/EP", calls[0].args[0])
        self.assertTrue(calls[1].args[0][0].endswith("probe.exe"))
        self.assertEqual(calls[1].kwargs["env"], {"_CL_": "mapped flags"})

    def test_ignored_mapping_is_rejected_without_logging_the_path(self):
        results = [
            subprocess.CompletedProcess("cl", 0, "probe.c", "warning D9002"),
            subprocess.CompletedProcess(
                "probe.exe", 0, r"C:\private-checkout\probe.c", ""
            ),
        ]
        with mock.patch.object(DEV.subprocess, "run", side_effect=results):
            with self.assertRaisesRegex(SystemExit, "D9002; marker absolute") as error:
                DEV.verify_msvc_path_mapping({})
        self.assertNotIn("private-checkout", str(error.exception))

    def test_failed_compiler_never_runs_a_probe_executable(self):
        completed = subprocess.CompletedProcess("cl", 1, "", "error C1083")
        with mock.patch.object(
            DEV.subprocess, "run", return_value=completed
        ) as compiler:
            with self.assertRaisesRegex(SystemExit, "compiler exit 1; codes C1083"):
                DEV.verify_msvc_path_mapping({})
        compiler.assert_called_once()

    def test_trimfile_is_selected_only_after_its_exact_path_probe(self):
        def compile_trial(args, **kwargs):
            if "/d1trimfile:" in kwargs["env"]["_CL_"]:
                return self.result(args, trim=True, **kwargs)
            if len(args) > 1:
                return subprocess.CompletedProcess(args, 0, "probe.c", "warning D9002")
            return subprocess.CompletedProcess(args, 0, "unmapped.c", "")

        with mock.patch.object(DEV.subprocess, "run", side_effect=compile_trial):
            with mock.patch("builtins.print"):
                actual = DEV.verified_msvc_environment({}, [(DEV.ROOT, "/echocord")])
        self.assertIn("/d1trimfile:", actual["_CL_"])
        self.assertNotIn("/pathmap:", actual["_CL_"])

    def test_partial_or_missing_trimfile_result_is_rejected(self):
        completed = subprocess.CompletedProcess("probe.exe", 0, "probe.c", "")
        with mock.patch.object(DEV.subprocess, "run", return_value=completed):
            with self.assertRaisesRegex(SystemExit, "unexpected relative"):
                DEV.verify_msvc_path_mapping({}, trim=True)

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
            '/DUSER_BUILD "/pathmap:C:\\workspace with spaces\\echocord=echocord"',
        )


if __name__ == "__main__":
    unittest.main()
