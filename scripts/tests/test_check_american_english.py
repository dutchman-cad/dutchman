import importlib.util
import pathlib
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parent.parent / "check-american-english.py"
spec = importlib.util.spec_from_file_location("check_american_english", SCRIPT)
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)


def words(text):
    return [w for _, _, w in check.spans(text)]


def literals(source):
    return [source[a:b] for _, a, b in check.string_literals(source)]


class AmericanEnglishCheck(unittest.TestCase):
    def test_british_spellings_are_found(self):
        self.assertEqual(words("The colour of the centre"), ["colour", "centre"])
        self.assertEqual(words("It was cancelled; a grey behaviour"), ["cancelled", "grey", "behaviour"])
        self.assertEqual(words("recognised, normalising, initialised, organisation"), ["recognis", "normalis", "initialis", "organis"])
        self.assertEqual(words("a licence in millimetres, one metre"), ["licence", "millimetre", "metre"])

    def test_american_spellings_and_other_words_pass(self):
        self.assertEqual(words("color center behavior recognized license gray canceled modeling millimeter meter"), [])
        self.assertEqual(words("an optimistic forecast; the greyhound; a metrenome"), [])

    def test_a_step_keyword_is_not_text(self):
        self.assertEqual(words("COLOUR_RGB('',1.,1.,1.) SI_UNIT(.MILLI.,.METRE.)"), [])

    def test_only_string_literals_of_rust_are_read(self):
        source = 'let a = "the colour"; // the centre\n/* a grey */ let b = r#"its "behaviour""#; let c = \'x\';\n'
        self.assertEqual(literals(source), ["the colour", 'its "behaviour"'])

    def test_a_literal_reports_its_own_line(self):
        source = 'fn f() {\n    let a = 1;\n    let b = "colour";\n}\n'
        self.assertEqual([n for n, _, _ in check.string_literals(source)], [3])

    def test_the_allowlist_covers_only_its_fragment(self):
        allow = {"a.md": ["replay.centre"], "b.rs": ["=cancelled"]}
        text = "the replay.centre key and a centre"
        first, second = list(check.spans(text))
        self.assertTrue(check.is_allowed("a.md", text, first[0], first[1], allow))
        self.assertFalse(check.is_allowed("a.md", text, second[0], second[1], allow))
        self.assertTrue(check.is_allowed("b.rs", "cancelled", 0, 9, allow))
        self.assertFalse(check.is_allowed("b.rs", "was cancelled", 4, 13, allow))

    def test_every_allowlist_line_has_a_reason(self):
        self.assertTrue(check.allowed())

    def test_the_repository_is_clean(self):
        used = set()
        self.assertEqual(check.violations(check.hits(), check.allowed(), used), [])
        stale = [(n, f) for n, fs in check.allowed().items() for f in fs if (n, f) not in used]
        self.assertEqual(stale, [])


if __name__ == "__main__":
    unittest.main()
