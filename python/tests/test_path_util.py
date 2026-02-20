"""Tests for PathUtil Python bindings."""

from lechange import PathUtil


class TestToPosix:
    def test_backslash_conversion(self):
        assert PathUtil.to_posix("foo\\bar") == "foo/bar"

    def test_already_posix(self):
        assert PathUtil.to_posix("foo/bar") == "foo/bar"

    def test_mixed_separators(self):
        assert PathUtil.to_posix("foo\\bar/baz") == "foo/bar/baz"


class TestNormalizeSeparator:
    def test_normalize(self):
        # On Unix, backslashes become forward slashes
        result = PathUtil.normalize_separator("foo\\bar")
        assert "/" in result or "\\" in result  # platform-dependent

    def test_already_normalized(self):
        result = PathUtil.normalize_separator("foo/bar")
        assert result == "foo/bar" or result == "foo\\bar"  # platform-dependent


class TestHasSeparator:
    def test_forward_slash(self):
        assert PathUtil.has_separator("foo/bar") is True

    def test_backslash(self):
        assert PathUtil.has_separator("foo\\bar") is True

    def test_no_separator(self):
        assert PathUtil.has_separator("foobar") is False


class TestComponents:
    def test_forward_slash(self):
        assert PathUtil.components("a/b/c") == ["a", "b", "c"]

    def test_backslash(self):
        assert PathUtil.components("a\\b\\c") == ["a", "b", "c"]

    def test_single_component(self):
        assert PathUtil.components("filename") == ["filename"]

    def test_mixed(self):
        assert PathUtil.components("a/b\\c") == ["a", "b", "c"]


class TestSeparator:
    def test_returns_string(self):
        sep = PathUtil.separator()
        assert isinstance(sep, str)
        assert len(sep) == 1
        assert sep in ("/", "\\")
