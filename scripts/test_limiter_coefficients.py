"""Keep the fixed detector artifact reproducible without external DSP packages."""

from decimal import localcontext
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location(
    "limiter_coefficients", Path(__file__).with_name("generate-limiter-coefficients.py")
)
GENERATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GENERATOR)


class LimiterCoefficientsTests(unittest.TestCase):
    def test_committed_coefficients_match_generator(self):
        output = Path(__file__).resolve().parents[1] / "crates/core/src/limiter/coefficients.rs"
        self.assertEqual(output.read_bytes(), GENERATOR.generate().encode("utf-8"))

    def test_extra_precision_does_not_change_rounded_coefficients(self):
        expected = GENERATOR.generate()
        with localcontext() as context:
            context.prec = 90
            self.assertEqual(GENERATOR.generate_at_current_precision(), expected)


if __name__ == "__main__":
    unittest.main()
