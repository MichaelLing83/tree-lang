"""Minimal integration tests for the tree_lang Python extension."""

import unittest

import tree_lang


class TestTreeLangPyMinimal(unittest.TestCase):
    def test_supported_languages(self) -> None:
        langs = tree_lang.supported_languages()
        self.assertIsInstance(langs, list)
        self.assertIn("rust", langs)

    def test_find_in_source_rust_function_definition(self) -> None:
        matches = tree_lang.find_in_source("fn f() {}", "rust", "function_definition")
        self.assertEqual(len(matches), 1)
        self.assertEqual(matches[0]["kind"], "FunctionDefinition")
        self.assertIn("start_line", matches[0])
        self.assertIn("content", matches[0])


if __name__ == "__main__":
    unittest.main()
