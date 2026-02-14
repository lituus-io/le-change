"""Tests for format utility functions."""

from lechange import escape_json, safe_output_escape, format_json_array, format_matrix


class TestEscapeJson:
    def test_no_op(self):
        assert escape_json("hello") == "hello"

    def test_quotes(self):
        assert escape_json('he"llo') == 'he\\"llo'

    def test_backslashes(self):
        assert escape_json("path\\to\\file") == "path\\\\to\\\\file"

    def test_newlines(self):
        assert escape_json("line1\nline2") == "line1\\nline2"

    def test_tabs(self):
        assert escape_json("col1\tcol2") == "col1\\tcol2"

    def test_carriage_return(self):
        assert escape_json("a\rb") == "a\\rb"


class TestSafeOutputEscape:
    def test_no_op(self):
        assert safe_output_escape("hello") == "hello"

    def test_percent(self):
        assert safe_output_escape("a%b") == "a%25b"

    def test_newline(self):
        assert safe_output_escape("a\nb") == "a%0Ab"

    def test_carriage_return(self):
        assert safe_output_escape("a\rb") == "a%0Db"


class TestFormatJsonArray:
    def test_empty(self):
        assert format_json_array([]) == "[]"

    def test_values(self):
        assert format_json_array(["a", "b"]) == '["a","b"]'

    def test_escaping(self):
        result = format_json_array(['he"llo'])
        assert '\\"' in result
        assert result.startswith("[")
        assert result.endswith("]")


class TestFormatMatrix:
    def test_empty(self):
        assert format_matrix([]) == '{"include":[]}'

    def test_single(self):
        assert format_matrix(["a"]) == '{"include":[{"value":"a"}]}'

    def test_multiple(self):
        result = format_matrix(["a", "b"])
        assert '{"value":"a"}' in result
        assert '{"value":"b"}' in result
        assert result.startswith('{"include":[')
        assert result.endswith("]}")
