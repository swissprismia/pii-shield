from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from presidio_sidecar import sanitize_json_value, sanitize_unicode_string


class JsonSanitizationTests(unittest.TestCase):
    def test_sanitize_unicode_string_replaces_lone_surrogates(self):
        value = f"table cell {chr(0xD83D)} markdown"

        sanitized = sanitize_unicode_string(value)

        self.assertEqual(sanitized, "table cell \ufffd markdown")

    def test_sanitize_unicode_string_repairs_valid_surrogate_pairs(self):
        value = f"emoji {chr(0xD83D)}{chr(0xDE00)} ok"

        sanitized = sanitize_unicode_string(value)

        self.assertEqual(sanitized, f"emoji {chr(0x1F600)} ok")

    def test_sanitize_json_value_recurses_through_response_shape(self):
        payload = {
            "success": True,
            "tokenized_text": f"| col |\n| --- |\n| {chr(0xD83D)} |",
            "entities": [
                {
                    "entity_type": "PERSON",
                    "text": f"A{chr(0xD83D)}B",
                }
            ],
            "token_map": {
                "Name1": f"Jane{chr(0xD83D)}",
            },
        }

        sanitized = sanitize_json_value(payload)

        replacement = chr(0xFFFD)
        self.assertTrue(sanitized["tokenized_text"].endswith(f"| {replacement} |"))
        self.assertEqual(sanitized["entities"][0]["text"], f"A{replacement}B")
        self.assertEqual(sanitized["token_map"]["Name1"], f"Jane{replacement}")


if __name__ == "__main__":
    unittest.main()
