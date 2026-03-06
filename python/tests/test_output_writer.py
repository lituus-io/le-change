"""Tests for OutputWriter Python bindings."""

import os
import pytest
from lechange import OutputWriter


class TestWriteText:
    def test_basic(self, tmp_path):
        OutputWriter.write_text(str(tmp_path), "files", ["a.rs", "b.rs", "c.rs"], "\n")
        content = (tmp_path / "files.txt").read_text()
        assert content == "a.rs\nb.rs\nc.rs"

    def test_custom_separator(self, tmp_path):
        OutputWriter.write_text(str(tmp_path), "files", ["a.rs", "b.rs"], ",")
        content = (tmp_path / "files.txt").read_text()
        assert content == "a.rs,b.rs"

    def test_empty(self, tmp_path):
        OutputWriter.write_text(str(tmp_path), "empty", [], "\n")
        content = (tmp_path / "empty.txt").read_text()
        assert content == ""


class TestWriteJson:
    def test_basic(self, tmp_path):
        OutputWriter.write_json(str(tmp_path), "files", ["a.rs", "b.rs"])
        content = (tmp_path / "files.json").read_text()
        assert content == '["a.rs","b.rs"]'

    def test_empty(self, tmp_path):
        OutputWriter.write_json(str(tmp_path), "files", [])
        content = (tmp_path / "files.json").read_text()
        assert content == "[]"


class TestErrors:
    def test_invalid_directory(self):
        with pytest.raises(OSError):
            OutputWriter.write_text("/nonexistent/path/xyz", "files", ["a"], "\n")
